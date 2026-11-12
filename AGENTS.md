# Regras do projeto FastTube

## Regra absoluta: testes e validações

Em toda tarefa ou prompt neste projeto, consulte e aplique esta regra antes de planejar ou executar qualquer trabalho. Toda atividade de teste ou validação, inclusive testes e validações associados à adição ou alteração de funcionalidades, deve ser delegada a um subagente **GPT-6 Luna com esforço high** ou **Gemini 3.8 Flash com esforço high**. Outros modelos e o agente principal não devem executar essas atividades. Se nenhum dos modelos permitidos estiver disponível, informe o bloqueio e não substitua o modelo sem autorização explícita do usuário.

Outros modelos, incluindo Gemini 4 Argon com esforço medium, podem realizar análise, planejamento e implementação, desde que não executem testes ou validações. As medições de desempenho devem ficar para a última etapa, conforme a orientação do usuário.

O subagente delegado que já estiver executando como GPT-6 Luna high ou Gemini 3.8 Flash high deve realizar pessoalmente os testes e as validações atribuídos a ele; não precisa delegá-los novamente. A delegação exigida é a feita pelo agente principal ao subagente autorizado.
