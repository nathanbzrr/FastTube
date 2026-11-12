# Análise de equivalência com o YouTube

Data da inspeção: 11 de novembro de 2026.

A inspeção foi feita com o site público do YouTube no navegador, em 1280×800 e 390×844, cobrindo Início, busca, reprodução, Shorts, canal, navegação expansível e a área Você. A implementação do FastTube foi comparada tanto pela interface executada quanto pelo código egui atual.

## Estrutura observada no YouTube

- Cabeçalho desktop de 56 px, busca central com largura máxima próxima de 640 px e ações compactas de voz, configurações e conta.
- Navegação recolhida com 72 px; ao expandir, vira um painel sobreposto de aproximadamente 240 px.
- Conteúdo com margem lateral de 24 px. A grade inicial alterna entre quatro colunas desktop, miniaturas 16:9 arredondadas e metadados em três níveis: título, canal e visualizações/data.
- Filtros em uma única faixa horizontal, sem quebra de linha vertical.
- Busca com miniatura panorâmica à esquerda, detalhes e descrição à direita; em telas estreitas, os cartões viram colunas empilhadas.
- Reprodução com vídeo e recomendações lado a lado em desktop, player acompanhando a altura útil e ações de Gostei, Não gostei, Compartilhar e Salvar próximas ao canal.
- Shorts em palco vertical central, com ações e informações laterais; nenhum contêiner horizontal 16:9.
- Canal com banner, identidade, linha de inscritos/vídeos, abas sublinhadas e prateleiras de conteúdo.
- Em janelas estreitas, a marca e os acessos globais ocupam uma linha curta, o conteúdo perde a navegação lateral e as grades se reduzem.

## Alterações aplicadas ao FastTube

- Medidas-base aproximadas: cabeçalho de 56 px, rail de 72 px, painel expandido de 216 a 240 px, margem de reprodução de 16 px e conteúdo com margem de 24 px.
- Navegação principal reorganizada como Início, Shorts, Inscrições e Explorar; o rail recolhido deixou de destacar o item ativo com um bloco grande.
- Filtros passaram a rolar horizontalmente, preservando a altura da página e evitando quebra em várias linhas.
- Cartões ganharam escala tipográfica mais próxima do YouTube, títulos adaptados à largura e ação de fila menos dominante.
- Página de canal recebeu metadados agrupados, abas sem caixas e indicador de seleção por sublinhado.
- Página Você/Biblioteca foi reconstruída com prateleiras horizontais para Assistir depois, Favoritos e Coleções, além de estados vazios e marcadores navegáveis.
- Shorts agora usam player vertical dedicado; em desktop, título, canal, descrição, compartilhamento e salvamento aparecem ao lado.
- Gostei e Não gostei foram conectados ao endpoint `videos.rate` da YouTube Data API, com processamento em segundo plano e atualização visual após a resposta.

## Lacunas restantes

A interface não replica anúncios, notificações, comentários com escrita, inscrição/desinscrição, gerenciamento remoto de playlists, chat ao vivo, downloads oficiais, Premium/Movies ou personalização de uma conta Google real em todos os detalhes. O FastTube também mantém sua identidade e seus recursos locais, como o histórico explicável, o perfil de recomendação e o diagnóstico do player.

Para uma próxima etapa, as maiores diferenças de produto são:

1. inscrição e desinscrição pela conta conectada;
2. envio de comentários e respostas;
3. navegação vertical contínua nos Shorts com pré-carregamento;
4. notificações e preferências da conta;
5. edição remota de playlists;
6. página Explorar alimentada por tendências oficiais por região.

Esses itens exigem operações adicionais na YouTube Data API e tratamento de cota, escopos e estados de conta. A camada visual atual já oferece a estrutura necessária para recebê-los sem alterar novamente toda a navegação.
