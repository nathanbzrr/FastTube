use std::io::{self, Read};
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[cfg(windows)]
mod win32_job {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub(super) const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    pub(super) const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    #[allow(dead_code)]
    pub(super) struct JobObjectBasicLimitInformation {
        pub per_process_user_time_limit: i64,
        pub per_job_user_time_limit: i64,
        pub limit_flags: u32,
        pub minimum_working_set_size: usize,
        pub maximum_working_set_size: usize,
        pub active_process_limit: u32,
        pub affinity: usize,
        pub priority_class: u32,
        pub scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    #[allow(dead_code)]
    pub(super) struct IoCounters {
        pub read_operation_count: u64,
        pub write_operation_count: u64,
        pub other_operation_count: u64,
        pub read_transfer_count: u64,
        pub write_transfer_count: u64,
        pub other_transfer_count: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    #[allow(dead_code)]
    pub(super) struct JobObjectExtendedLimitInformation {
        pub basic_limit_information: JobObjectBasicLimitInformation,
        pub io_info: IoCounters,
        pub process_memory_limit: usize,
        pub job_memory_limit: usize,
        pub peak_process_memory_used: usize,
        pub peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(lp_job_attributes: *const c_void, lp_name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(
            h_job: *mut c_void,
            job_object_information_class: i32,
            lp_job_object_information: *const c_void,
            cb_job_object_information_length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(h_job: *mut c_void, h_process: *mut c_void) -> i32;
        fn TerminateJobObject(h_job: *mut c_void, u_exit_code: u32) -> i32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
    }

    /// RAII wrapper around a Win32 Job Object configured with
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so child process trees terminate
    /// cleanly on cancellation or parent exit.
    pub struct ProcessJobObject {
        handle: *mut c_void,
        terminated: AtomicBool,
    }

    // Win32 Job Object handles are safe to share across threads for
    // `AssignProcessToJobObject` and `TerminateJobObject` while `CloseHandle`
    // only executes once on final `Drop`.
    unsafe impl Send for ProcessJobObject {}
    unsafe impl Sync for ProcessJobObject {}

    impl ProcessJobObject {
        pub fn new() -> Option<Self> {
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                return None;
            }
            let mut info = JobObjectExtendedLimitInformation::default();
            info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    handle,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    (&raw const info).cast::<c_void>(),
                    std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
                )
            };
            if configured == 0 {
                unsafe {
                    CloseHandle(handle);
                }
                return None;
            }
            Some(Self {
                handle,
                terminated: AtomicBool::new(false),
            })
        }

        pub fn assign_child(&self, child: &Child) -> bool {
            if self.handle.is_null() {
                return false;
            }
            let proc_handle = child.as_raw_handle();
            unsafe { AssignProcessToJobObject(self.handle, proc_handle) != 0 }
        }

        pub fn terminate(&self) {
            if self.handle.is_null() {
                return;
            }
            if !self.terminated.swap(true, Ordering::AcqRel) {
                unsafe {
                    TerminateJobObject(self.handle, 1);
                }
            }
        }
    }

    impl Drop for ProcessJobObject {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    CloseHandle(self.handle);
                }
                self.handle = ptr::null_mut();
            }
        }
    }
}

#[cfg(windows)]
pub use win32_job::ProcessJobObject;

#[cfg(not(windows))]
pub struct ProcessJobObject;

#[cfg(not(windows))]
impl ProcessJobObject {
    pub fn new() -> Option<Self> {
        Some(Self)
    }

    pub fn assign_child(&self, _child: &Child) -> bool {
        false
    }

    pub fn terminate(&self) {}
}

fn read_output(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut buffer = [0; 8192];
    let mut oversized = false;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(data.len());
        data.extend_from_slice(&buffer[..count.min(remaining)]);
        oversized |= count > remaining;
    }
    if oversized {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saída da consulta excede o limite",
        ))
    } else {
        Ok(data)
    }
}

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

pub fn output(
    command: &mut Command,
    cancellation: &Cancellation,
    timeout: Duration,
) -> io::Result<Output> {
    if cancellation.cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "consulta cancelada",
        ));
    }
    let job = ProcessJobObject::new();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(ref job) = job {
        job.assign_child(&child);
    }
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = out_tx.send(read_output(stdout));
    });
    std::thread::spawn(move || {
        let _ = err_tx.send(read_output(stderr));
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if cancellation.cancelled() || start.elapsed() >= timeout => {
                if let Some(ref job) = job {
                    job.terminate();
                }
                let _ = child.kill();
                let _ = child.wait();
                break Err(io::Error::new(
                    if cancellation.cancelled() {
                        io::ErrorKind::Interrupted
                    } else {
                        io::ErrorKind::TimedOut
                    },
                    if cancellation.cancelled() {
                        "consulta cancelada"
                    } else {
                        "tempo limite da consulta excedido"
                    },
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                if let Some(ref job) = job {
                    job.terminate();
                }
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
        }
    };
    // Descendants may inherit the pipes. Terminate the job on timeout/cancel so they cannot stall.
    let status = status?;
    let receive = |receiver: std::sync::mpsc::Receiver<io::Result<Vec<u8>>>| loop {
        if cancellation.cancelled() {
            if let Some(ref job) = job {
                job.terminate();
            }
            break Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "consulta cancelada",
            ));
        }
        if start.elapsed() >= timeout {
            if let Some(ref job) = job {
                job.terminate();
            }
            break Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "tempo limite da consulta excedido",
            ));
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => break result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break Err(io::Error::other("falha ao ler saída")),
        }
    };
    let stdout = receive(out_rx)?;
    let stderr = receive(err_rx)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};
    use std::os::windows::process::CommandExt;

    #[test]
    fn job_object_extended_limit_info_layout_matches_win32_abi() {
        assert_eq!(size_of::<win32_job::IoCounters>(), 48);
        assert_eq!(align_of::<win32_job::IoCounters>(), 8);
        assert_eq!(win32_job::JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS, 9);
        assert_eq!(win32_job::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0x2000);

        #[cfg(target_arch = "x86_64")]
        {
            assert_eq!(size_of::<win32_job::JobObjectBasicLimitInformation>(), 64);
            assert_eq!(align_of::<win32_job::JobObjectBasicLimitInformation>(), 8);
            assert_eq!(
                offset_of!(win32_job::JobObjectBasicLimitInformation, limit_flags),
                16
            );
            assert_eq!(
                offset_of!(
                    win32_job::JobObjectBasicLimitInformation,
                    minimum_working_set_size
                ),
                24
            );
            assert_eq!(
                offset_of!(
                    win32_job::JobObjectBasicLimitInformation,
                    maximum_working_set_size
                ),
                32
            );
            assert_eq!(
                offset_of!(
                    win32_job::JobObjectBasicLimitInformation,
                    active_process_limit
                ),
                40
            );
            assert_eq!(
                offset_of!(win32_job::JobObjectBasicLimitInformation, affinity),
                48
            );
            assert_eq!(
                offset_of!(win32_job::JobObjectBasicLimitInformation, priority_class),
                56
            );
            assert_eq!(
                offset_of!(win32_job::JobObjectBasicLimitInformation, scheduling_class),
                60
            );

            assert_eq!(
                size_of::<win32_job::JobObjectExtendedLimitInformation>(),
                144
            );
            assert_eq!(
                align_of::<win32_job::JobObjectExtendedLimitInformation>(),
                8
            );
            assert_eq!(
                offset_of!(win32_job::JobObjectExtendedLimitInformation, io_info),
                64
            );
            assert_eq!(
                offset_of!(
                    win32_job::JobObjectExtendedLimitInformation,
                    process_memory_limit
                ),
                112
            );
            assert_eq!(
                offset_of!(
                    win32_job::JobObjectExtendedLimitInformation,
                    peak_job_memory_used
                ),
                136
            );
        }
    }

    #[test]
    fn job_object_creates_assigns_and_terminates_child_tree() {
        let job = ProcessJobObject::new().expect("CreateJobObjectW + SetInformationJobObject");
        let mut cmd = Command::new("ping");
        cmd.args(["-n", "10", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000);
        let mut child = cmd.spawn().expect("spawn ping");
        assert!(
            job.assign_child(&child),
            "AssignProcessToJobObject should succeed"
        );
        let started = Instant::now();
        job.terminate();
        let status = child.wait().expect("wait for terminated child");
        assert!(!status.success());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn timeout_and_cancellation_stop_the_child() {
        let mut slow = Command::new("ping");
        slow.args(["-n", "10", "127.0.0.1"]);
        let started = Instant::now();
        assert_eq!(
            output(
                &mut slow,
                &Cancellation::default(),
                Duration::from_millis(150)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        let cancellation = Cancellation::default();
        let cancel_from_thread = cancellation.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            cancel_from_thread.cancel();
        });
        let mut slow = Command::new("ping");
        slow.args(["-n", "10", "127.0.0.1"]);
        assert_eq!(
            output(&mut slow, &cancellation, Duration::from_secs(5))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        canceller.join().unwrap();
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert_eq!(
            output(
                &mut Command::new("ping"),
                &cancellation,
                Duration::from_secs(1)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
    }
}
