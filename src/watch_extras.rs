use crate::{history, oauth, process, search};
use eframe::egui;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

static SUBTITLE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);
const SUBTITLE_VTT_CACHE_LIMIT: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadySubtitle {
    pub url: String,
    pub path: PathBuf,
    pub label: String,
    pub language: String,
}

struct DownloadedSubtitle {
    url: String,
    label: String,
    language: String,
    vtt: String,
}

struct TranscriptPayload {
    url: String,
    vtt: String,
    cues: Vec<crate::transcript::Cue>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Results,
    Comments,
    Subtitles,
    Markers,
}

#[derive(Default)]
pub struct CommentReplies {
    pub comments: Vec<oauth::Comment>,
    pub next: Option<String>,
    pub error: Option<String>,
    pub loaded: bool,
    loading: Option<Receiver<Result<oauth::CommentsPage, String>>>,
    cancel: Option<process::Cancellation>,
}

impl CommentReplies {
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    pub fn retry_uses_next_page(&self) -> bool {
        self.next.is_some()
    }
}

#[derive(Default)]
struct TempSubtitleFiles(HashMap<String, PathBuf>);

impl Drop for TempSubtitleFiles {
    fn drop(&mut self) {
        for (_, path) in self.0.drain() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Default)]
pub struct WatchExtras {
    pub video_url: Option<String>,
    pub tab: Tab,
    pub comments: Vec<oauth::Comment>,
    pub comments_next: Option<String>,
    pub comments_error: Option<String>,
    pub comments_loaded: bool,
    pub comments_loading: Option<Receiver<Result<oauth::CommentsPage, String>>>,
    comments_cancel: Option<process::Cancellation>,
    pub comment_replies: HashMap<String, CommentReplies>,
    pub subtitles: Vec<search::SubtitleTrack>,
    pub subtitles_error: Option<String>,
    pub subtitles_loaded: bool,
    pub subtitles_loading: Option<Receiver<Result<Vec<search::SubtitleTrack>, String>>>,
    subtitles_cancel: Option<process::Cancellation>,
    pub active_subtitle_url: Option<String>,
    vtt_cache: HashMap<String, String>,
    temp_subtitle_files: TempSubtitleFiles,
    subtitle_apply_rx: Option<Receiver<Result<DownloadedSubtitle, String>>>,
    subtitle_apply_cancel: Option<process::Cancellation>,
    ready_subtitle: Option<ReadySubtitle>,
    pub transcript: Vec<crate::transcript::Cue>,
    pub transcript_query: String,
    pub transcript_error: Option<String>,
    pub transcript_loading: bool,
    pub transcript_follow: bool,
    pub transcript_font_size: f32,
    pub transcript_high_contrast: bool,
    transcript_rx: Option<Receiver<Result<TranscriptPayload, String>>>,
    transcript_cancel: Option<process::Cancellation>,
}

impl WatchExtras {
    pub fn sync_video(&mut self, video_url: Option<&str>) {
        if self.video_url.as_deref() == video_url {
            return;
        }
        if let Some(cancel) = self.subtitles_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.subtitle_apply_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.comments_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.transcript_cancel.take() {
            cancel.cancel();
        }
        for replies in self.comment_replies.values_mut() {
            if let Some(cancel) = replies.cancel.take() {
                cancel.cancel();
            }
        }
        let transcript_follow = self.transcript_follow;
        let transcript_font_size = self.transcript_font_size;
        let transcript_high_contrast = self.transcript_high_contrast;
        *self = Self {
            video_url: video_url.map(str::to_owned),
            transcript_follow,
            transcript_font_size,
            transcript_high_contrast,
            ..Self::default()
        };
    }

    pub fn reset_comments(&mut self) {
        if let Some(cancel) = self.comments_cancel.take() {
            cancel.cancel();
        }
        for replies in self.comment_replies.values_mut() {
            if let Some(cancel) = replies.cancel.take() {
                cancel.cancel();
            }
        }
        self.comments.clear();
        self.comments_next = None;
        self.comments_error = None;
        self.comments_loaded = false;
        self.comments_loading = None;
        self.comment_replies.clear();
    }

    pub fn start_comments(
        &mut self,
        session: Option<&oauth::TokenSession>,
        next: bool,
        ctx: &egui::Context,
    ) {
        if self.comments_loading.is_some() {
            return;
        }
        let Some(mut session) = session.cloned() else {
            self.comments_error = Some("Entre novamente para consultar comentários.".into());
            return;
        };
        if !session.has_comment_scope() {
            self.comments_error = Some(
                "A sessão conectada não tem permissão para ler comentários. Use \"Autorizar comentários\".".to_owned(),
            );
            return;
        }
        let Some(video_id) = self
            .video_url
            .as_deref()
            .and_then(history::youtube_video_id)
        else {
            self.comments_error =
                Some("Comentários disponíveis somente para vídeos do YouTube.".into());
            return;
        };
        let page = if next {
            self.comments_next.clone()
        } else {
            None
        };
        if next && page.is_none() {
            return;
        }
        if !next {
            self.comments.clear();
            self.comments_next = None;
        }
        self.comments_error = None;
        let cancel = process::Cancellation::default();
        self.comments_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.comments_loading = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = session.refresh_if_needed(&cancel).and_then(|()| {
                oauth::comments_page(&session.access_token, &video_id, page.as_deref(), &cancel)
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn start_replies(
        &mut self,
        comment_id: &str,
        next: bool,
        session: Option<&oauth::TokenSession>,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = session.cloned() else {
            self.comment_replies
                .entry(comment_id.to_owned())
                .or_default()
                .error = Some("Entre novamente para consultar respostas.".into());
            return;
        };
        let state = self
            .comment_replies
            .entry(comment_id.to_owned())
            .or_default();
        if state.loading.is_some() {
            return;
        }
        let page = if next { state.next.clone() } else { None };
        if next && page.is_none() {
            return;
        }
        if !next {
            state.comments.clear();
            state.next = None;
        }
        state.error = None;
        let cancel = process::Cancellation::default();
        state.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        state.loading = Some(receiver);
        let parent_id = comment_id.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = session.refresh_if_needed(&cancel).and_then(|()| {
                oauth::comment_replies_page(
                    &session.access_token,
                    &parent_id,
                    page.as_deref(),
                    &cancel,
                )
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
    pub fn start_subtitles(&mut self, ctx: &egui::Context) {
        if self.subtitles_loading.is_some() {
            return;
        }
        let Some(url) = self.video_url.clone() else {
            return;
        };
        let cancel = process::Cancellation::default();
        self.subtitles_cancel = Some(cancel.clone());
        self.subtitles_error = None;
        let (sender, receiver) = mpsc::channel();
        self.subtitles_loading = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(search::youtube_subtitles(&url, &cancel));
            ctx.request_repaint();
        });
    }

    fn cache_vtt(&mut self, url: &str, vtt: String) {
        if self.vtt_cache.len() >= SUBTITLE_VTT_CACHE_LIMIT && !self.vtt_cache.contains_key(url) {
            self.vtt_cache.clear();
        }
        self.vtt_cache.insert(url.to_owned(), vtt);
    }

    fn ensure_temp_subtitle_file(&mut self, url: &str, vtt: &str) -> Result<PathBuf, String> {
        if let Some(existing) = self.temp_subtitle_files.0.get(url)
            && existing.exists()
        {
            return Ok(existing.clone());
        }
        let dir = std::env::temp_dir();
        for _ in 0..8 {
            let seq = SUBTITLE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("fasttube-sub-{}-{seq}.vtt", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    if file.write_all(vtt.as_bytes()).is_err() {
                        let _ = std::fs::remove_file(&path);
                        return Err(
                            "Não foi possível gravar o arquivo temporário de legenda.".into()
                        );
                    }
                    self.temp_subtitle_files
                        .0
                        .insert(url.to_owned(), path.clone());
                    return Ok(path);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => {
                    return Err("Não foi possível criar o arquivo temporário de legenda.".into());
                }
            }
        }
        Err("Não foi possível preparar o arquivo temporário de legenda.".into())
    }

    pub fn is_subtitle_applying(&self) -> bool {
        self.subtitle_apply_rx.is_some()
    }

    pub fn take_ready_subtitle(&mut self) -> Option<ReadySubtitle> {
        self.ready_subtitle.take()
    }

    pub fn start_apply_subtitle(
        &mut self,
        track: &search::SubtitleTrack,
        label: String,
        ctx: &egui::Context,
    ) {
        if let Some(cancel) = self.subtitle_apply_cancel.take() {
            cancel.cancel();
        }
        self.subtitle_apply_rx = None;
        self.ready_subtitle = None;
        self.subtitles_error = None;
        if let Some(vtt) = self.vtt_cache.get(&track.url).cloned() {
            match self.ensure_temp_subtitle_file(&track.url, &vtt) {
                Ok(path) => {
                    self.ready_subtitle = Some(ReadySubtitle {
                        url: track.url.clone(),
                        path,
                        label,
                        language: track.language.clone(),
                    });
                }
                Err(error) => self.subtitles_error = Some(error),
            }
            return;
        }
        let url = track.url.clone();
        let language = track.language.clone();
        let cancel = process::Cancellation::default();
        self.subtitle_apply_cancel = Some(cancel.clone());
        let (tx, rx) = mpsc::channel();
        self.subtitle_apply_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if cancel.cancelled() {
                Err("Download da legenda cancelado.".into())
            } else {
                search::download_vtt(&url).and_then(|vtt| {
                    let cues = crate::transcript::parse_vtt(&vtt).map_err(str::to_owned)?;
                    if cues.is_empty() {
                        return Err("A faixa selecionada não contém texto de legenda.".into());
                    }
                    Ok(DownloadedSubtitle {
                        url,
                        label,
                        language,
                        vtt,
                    })
                })
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub fn start_transcript(&mut self, track: &search::SubtitleTrack, ctx: &egui::Context) {
        if self.transcript_loading {
            return;
        }
        self.transcript_error = None;
        self.transcript.clear();
        self.transcript_query.clear();
        if let Some(vtt) = self.vtt_cache.get(&track.url) {
            match crate::transcript::parse_vtt(vtt).map_err(str::to_owned) {
                Ok(cues) if cues.is_empty() => {
                    self.transcript_error =
                        Some("A faixa selecionada não contém trechos de legenda.".into());
                }
                Ok(cues) => self.transcript = cues,
                Err(error) => self.transcript_error = Some(error),
            }
            return;
        }
        self.transcript_loading = true;
        let url = track.url.clone();
        let cancel = process::Cancellation::default();
        self.transcript_cancel = Some(cancel.clone());
        let (tx, rx) = mpsc::channel();
        self.transcript_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if cancel.cancelled() {
                Err("Consulta cancelada.".into())
            } else {
                search::download_vtt(&url).and_then(|vtt| {
                    let cues = crate::transcript::parse_vtt(&vtt).map_err(str::to_owned)?;
                    if cues.is_empty() {
                        return Err("A faixa selecionada não contém trechos de legenda.".into());
                    }
                    Ok(TranscriptPayload { url, vtt, cues })
                })
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self) {
        let subtitle_apply_result = self.subtitle_apply_rx.as_ref().map(Receiver::try_recv);
        match subtitle_apply_result {
            Some(Ok(Ok(downloaded))) => {
                self.cache_vtt(&downloaded.url, downloaded.vtt.clone());
                match self.ensure_temp_subtitle_file(&downloaded.url, &downloaded.vtt) {
                    Ok(path) => {
                        self.ready_subtitle = Some(ReadySubtitle {
                            url: downloaded.url,
                            path,
                            label: downloaded.label,
                            language: downloaded.language,
                        });
                    }
                    Err(error) => self.subtitles_error = Some(error),
                }
                self.subtitle_apply_rx = None;
                self.subtitle_apply_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.subtitles_error = Some(error);
                self.subtitle_apply_rx = None;
                self.subtitle_apply_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.subtitles_error = Some("Download da legenda interrompido.".into());
                self.subtitle_apply_rx = None;
                self.subtitle_apply_cancel = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
        let transcript_result = self.transcript_rx.as_ref().map(Receiver::try_recv);
        match transcript_result {
            Some(Ok(Ok(payload))) => {
                self.cache_vtt(&payload.url, payload.vtt);
                self.transcript = payload.cues;
                self.transcript_loading = false;
                self.transcript_rx = None;
                self.transcript_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.transcript_error = Some(error);
                self.transcript_loading = false;
                self.transcript_rx = None;
                self.transcript_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.transcript_error = Some("Consulta de transcrição interrompida.".into());
                self.transcript_loading = false;
                self.transcript_rx = None;
                self.transcript_cancel = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
        match self.comments_loading.as_ref().map(Receiver::try_recv) {
            Some(Ok(Ok(page))) => {
                self.comments_loaded = true;
                self.comments.extend(page.comments);
                self.comments.truncate(100);
                self.comments_next = if self.comments.len() < 100 {
                    page.next_page
                } else {
                    None
                };
                self.comments_loading = None;
                self.comments_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.comments_loaded = true;
                self.comments_error = Some(error);
                self.comments_loading = None;
                self.comments_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.comments_loaded = true;
                self.comments_error = Some("Consulta de comentários interrompida.".into());
                self.comments_loading = None;
                self.comments_cancel = None;
            }
            _ => {}
        }
        for replies in self.comment_replies.values_mut() {
            match replies.loading.as_ref().map(Receiver::try_recv) {
                Some(Ok(Ok(page))) => {
                    replies.loaded = true;
                    replies.comments.extend(page.comments);
                    replies.comments.truncate(100);
                    replies.next = if replies.comments.len() < 100 {
                        page.next_page
                    } else {
                        None
                    };
                    replies.loading = None;
                    replies.cancel = None;
                }
                Some(Ok(Err(error))) => {
                    replies.loaded = true;
                    replies.error = Some(error);
                    replies.loading = None;
                    replies.cancel = None;
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    replies.loaded = true;
                    replies.error = Some("Consulta de respostas interrompida.".into());
                    replies.loading = None;
                    replies.cancel = None;
                }
                _ => {}
            }
        }
        match self.subtitles_loading.as_ref().map(Receiver::try_recv) {
            Some(Ok(Ok(tracks))) => {
                self.subtitles = tracks;
                self.subtitles_loaded = true;
                self.subtitles_loading = None;
                self.subtitles_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.subtitles_loaded = true;
                self.subtitles_error = Some(error);
                self.subtitles_loading = None;
                self.subtitles_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.subtitles_loaded = true;
                self.subtitles_error = Some("Consulta de legendas interrompida.".into());
                self.subtitles_loading = None;
                self.subtitles_cancel = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_reply(id: &str) -> oauth::Comment {
        oauth::Comment {
            id: id.into(),
            author: "Ana".into(),
            text: "Resposta".into(),
            published: String::new(),
            likes: 0,
            replies: 0,
        }
    }

    #[test]
    fn changing_video_discards_pending_responses() {
        let mut state = WatchExtras::default();
        state.sync_video(Some("https://www.youtube.com/watch?v=one"));
        let (sender, receiver) = mpsc::channel();
        state.comments_loading = Some(receiver);
        let cancel = process::Cancellation::default();
        state.comments_cancel = Some(cancel.clone());
        state.sync_video(Some("https://www.youtube.com/watch?v=two"));
        assert!(cancel.cancelled());
        assert!(
            sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![],
                    next_page: None
                }))
                .is_err()
        );
        assert!(state.comments_loading.is_none());
        assert_eq!(
            state.video_url.as_deref(),
            Some("https://www.youtube.com/watch?v=two")
        );
    }

    #[test]
    fn missing_comment_scope_sets_error_without_loading() {
        let mut state = WatchExtras::default();
        state.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        state.start_comments(
            Some(&oauth::TokenSession::test_session_without_comment_scope(
                "old",
            )),
            false,
            &egui::Context::default(),
        );
        assert!(state.comments_loading.is_none());
        assert!(!state.comments_loaded);
        assert!(
            state
                .comments_error
                .as_deref()
                .unwrap()
                .contains("Autorizar coment\u{00e1}rios")
        );
    }

    #[test]
    fn reset_comments_clears_state_and_cancels_pending_requests() {
        let mut state = WatchExtras {
            comments: vec![oauth::Comment {
                id: "comment-1".into(),
                author: "Ana".into(),
                text: "texto".into(),
                published: String::new(),
                likes: 0,
                replies: 1,
            }],
            comments_next: Some("next".into()),
            comments_error: Some("erro".into()),
            comments_loaded: true,
            ..WatchExtras::default()
        };
        let comments_cancel = process::Cancellation::default();
        state.comments_cancel = Some(comments_cancel.clone());
        let (sender, receiver) = mpsc::channel();
        state.comments_loading = Some(receiver);
        let replies_cancel = process::Cancellation::default();
        let replies = state.comment_replies.entry("comment-1".into()).or_default();
        let (reply_sender, reply_receiver) = mpsc::channel();
        replies.loading = Some(reply_receiver);
        replies.cancel = Some(replies_cancel.clone());

        state.reset_comments();

        assert!(comments_cancel.cancelled());
        assert!(replies_cancel.cancelled());
        assert!(
            sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![],
                    next_page: None
                }))
                .is_err()
        );
        assert!(
            reply_sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![],
                    next_page: None
                }))
                .is_err()
        );
        assert!(state.comments.is_empty());
        assert!(state.comments_next.is_none());
        assert!(state.comments_error.is_none());
        assert!(!state.comments_loaded);
        assert!(state.comments_loading.is_none());
        assert!(state.comment_replies.is_empty());
    }

    #[test]
    fn comments_pages_append_and_stop_at_limit() {
        let mut state = WatchExtras::default();
        let comment = oauth::Comment {
            id: "comment-1".into(),
            author: "Ana".into(),
            text: "Olá".into(),
            published: String::new(),
            likes: 0,
            replies: 0,
        };
        state.comments = vec![comment.clone(); 99];
        let (sender, receiver) = mpsc::channel();
        state.comments_loading = Some(receiver);
        assert!(
            sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![comment; 2],
                    next_page: Some("next".into()),
                }))
                .is_ok()
        );
        state.poll();
        assert_eq!(state.comments.len(), 100);
        assert!(state.comments_next.is_none());
        assert!(state.comments_loaded);
    }

    #[test]
    fn failed_first_reply_page_can_retry_from_the_first_page() {
        let mut extras = WatchExtras::default();
        let (sender, receiver) = mpsc::channel();
        let state = extras
            .comment_replies
            .entry("comment-1".into())
            .or_default();
        state.loading = Some(receiver);
        sender.send(Err("falha temporária".into())).unwrap();

        extras.poll();

        let state = extras.comment_replies.get("comment-1").unwrap();
        assert!(state.loaded);
        assert_eq!(state.error.as_deref(), Some("falha temporária"));
        assert!(!state.retry_uses_next_page());
    }

    #[test]
    fn failed_later_reply_page_keeps_replies_and_retries_same_page() {
        let mut extras = WatchExtras::default();
        let (sender, receiver) = mpsc::channel();
        let state = extras
            .comment_replies
            .entry("comment-1".into())
            .or_default();
        state.comments = vec![sample_reply("reply-1")];
        state.next = Some("page-2".into());
        state.loaded = true;
        state.loading = Some(receiver);
        sender.send(Err("falha temporária".into())).unwrap();

        extras.poll();

        let state = extras.comment_replies.get("comment-1").unwrap();
        assert_eq!(state.comments.len(), 1);
        assert_eq!(state.comments[0].id, "reply-1");
        assert_eq!(state.next.as_deref(), Some("page-2"));
        assert_eq!(state.error.as_deref(), Some("falha temporária"));
        assert!(state.retry_uses_next_page());
    }

    #[test]
    fn changing_video_cancels_reply_request_and_discards_obsolete_result() {
        let mut state = WatchExtras::default();
        state.sync_video(Some("https://www.youtube.com/watch?v=one"));
        let cancel = process::Cancellation::default();
        let (sender, receiver) = mpsc::channel();
        let replies = state.comment_replies.entry("comment-1".into()).or_default();
        replies.loading = Some(receiver);
        replies.cancel = Some(cancel.clone());

        state.sync_video(Some("https://www.youtube.com/watch?v=two"));

        assert!(cancel.cancelled());
        assert!(
            sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![sample_reply("obsolete-reply")],
                    next_page: None,
                }))
                .is_err()
        );
        state.poll();
        assert!(!state.comment_replies.contains_key("comment-1"));
        assert_eq!(
            state.video_url.as_deref(),
            Some("https://www.youtube.com/watch?v=two")
        );
    }

    #[test]
    fn subtitle_cache_prepares_temp_file_and_cleans_up_on_video_change() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=one"));
        let track = search::SubtitleTrack {
            language: "pt-BR".into(),
            automatic: false,
            url: "https://www.youtube.com/api/timedtext?v=one&lang=pt-BR".into(),
        };
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nOlá mundo\n".to_owned();
        let (tx, rx) = mpsc::channel();
        extras.subtitle_apply_rx = Some(rx);
        tx.send(Ok(DownloadedSubtitle {
            url: track.url.clone(),
            label: "pt-BR".into(),
            language: "pt-BR".into(),
            vtt,
        }))
        .unwrap();

        extras.poll();

        let ready = extras
            .take_ready_subtitle()
            .expect("ready subtitle should be produced");
        assert!(ready.path.exists());
        let saved = std::fs::read_to_string(&ready.path).unwrap();
        assert!(saved.contains("Olá mundo"));

        // Reusing cache for transcript avoids network and parses cues immediately.
        extras.start_transcript(&track, &egui::Context::default());
        assert!(!extras.transcript_loading);
        assert_eq!(extras.transcript.len(), 1);
        assert_eq!(extras.transcript[0].text, "Olá mundo");

        // Switching video cleans up the temporary .vtt file.
        extras.sync_video(Some("https://www.youtube.com/watch?v=two"));
        assert!(!ready.path.exists());
    }
}
