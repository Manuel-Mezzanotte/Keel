# Keel — cosa fare dopo

*Scritto il 24 novembre 2026, sulla versione di `develop` col benvenuto del primo avvio; segnate il 27 novembre le idee fatte con la 0.1.3.*

Questa è la lista di cosa si potrebbe aggiungere o migliorare in Keel. Nasce da tre cose:
- **quello che Keel ha oggi**, letto nel codice e in `DETTAGLI.md` e `HANDOFF.md`;
- **le novità degli altri editor** negli ultimi mesi: VS Code fino alla 1.147 (18 novembre), Zed fino alla 1.29, Cursor 3, Xcode 27, IntelliJ IDEA 2026.3 e Warp;
- **cosa usano e chiedono gli sviluppatori**: il sondaggio di Stack Overflow 2026 e le richieste più votate di VS Code.

Le fonti sono in fondo. Niente qui è deciso: ogni voce parte solo col via dell'autore.

---

## Cosa dice la ricerca, in breve

- **VS Code resta il più usato** (70,9% nel sondaggio di Stack Overflow 2026, era 75,9% nel 2025). Cursor scende al 14,9%. Chi arriva su Keel arriva quasi sempre da VS Code.
- **Il centro si è spostato sugli agenti.** Claude Code è l'agente più usato nel sondaggio. VS Code, Cursor, Zed, Xcode 27 e Warp nell'ultimo anno hanno lavorato quasi solo su questo: più sessioni insieme, ognuna nel suo worktree, il piano da approvare prima, e soprattutto **rivedere bene quello che l'agente ha cambiato**.
- **L'editor diventa il posto dove si guida e si controlla l'agente.** È lo spazio giusto per Keel: leggero, nativo, con Claude Code già dentro.
- **La leggerezza conta ancora.** Le critiche a VS Code sono memoria e lentezza con tante estensioni. Il vantaggio di Keel (180 MB contro 748, rimisurato il 27 novembre) va difeso prima di tutto il resto.

## Come leggere la lista

- **Peso:** *piccolo* (un giorno o meno), *medio* (qualche giorno), *grande* (una o due settimane), *molto grande* (di più, o con parti non nostre).
- **Chi ce l'ha:** dove si è visto, così si può guardare come lo fanno.
- 🔸 vuol dire che serve una decisione dell'autore prima di cominciare.

---

## Le dieci che farei per prime

| # | Cosa | Perché adesso | Peso |
|---|---|---|---|
| 1 | ~~**Correggere la memoria grafica e la CPU da fermo**~~ **fatto il 24 novembre** | Da fermo, con un file aperto: CPU dal 7% allo 0%, memoria da 272 a 157 MB. Il racconto è in `HANDOFF.md` | — |
| 2 | ~~**Rivedere le modifiche di Claude come in Warp e Xcode 27**~~ **fatto con la 0.1.3** | L'elenco dei file toccati, le differenze, Accetta e Rifiuta per file, i commenti sulle righe e sui blocchi mandati a Claude tutti insieme | — |
| 3 | ~~**Importare da VS Code**~~ **fatto il 25 novembre** | Impostazioni, scorciatoie, frammenti, progetti recenti ed estensioni, da VS Code come da Cursor | — |
| 4 | **Go e Swift** | Keel non ha né server né colori per Go, Swift, PHP, Ruby, Kotlin, Vue e Svelte. Go è fra i linguaggi più usati; Swift è il linguaggio del Mac, e `sourcekit-lsp` c'è già con Xcode | medio |
| 5 | **Più sessioni di Claude insieme, ognuna nel suo worktree** | È la novità di Cursor 3, della finestra Agenti di VS Code e di Zed 1.0. Keel segue già i worktree, ma non li crea né li mostra | grande |
| 6 | ~~**Il piano di Claude da commentare, e tornare indietro**~~ **fatto con la 0.1.3** | Il piano in una scheda, cambiato e annotato e rimandato con «Rivedi con le mie note»; il codice riportato a un messaggio; le chat divise da una risposta | — |
| 7 | ~~**Dopo il benvenuto, tre scelte**~~ **fatto il 25 novembre** | «Prepara Keel»: tema, «Importa» da VS Code o Cursor, comando `keel` nel Terminale | — |
| 8 | **Un editor per i conflitti a tre colonne** | Oggi un file in conflitto si apre in una scheda con un pulsante per ogni conflitto (la corrente, quella in arrivo o entrambe) e «Salva e metti in stage» (fase 5.3). Manca la vista a tre colonne di VS Code e IntelliJ, con la base e il risultato, e i pulsanti dentro l'editor normale | medio |
| 9 | ~~**Firma e notarizzazione di Apple**~~ **non si fa**, per scelta dell'autore (25 novembre): niente account sviluppatore a 99 $ l'anno | Chi installa con lo script del sito (`curl … \| sh`) non vede l'avviso di macOS; chi scarica il DMG dal browser sì, e deve aprirlo da Impostazioni di Sistema › Privacy e sicurezza › «Apri comunque» | — |
| 10 | **Staccare pannelli e terminale in finestre loro** | È la richiesta più votata di sempre su VS Code, e la seconda è il terminale staccato. Su Keel c'è già in elenco la scheda trascinata fuori | grande |

---

## Claude e gli agenti

È l'area che si muove più in fretta. Keel parte bene: pannello di Claude Code con le tue chat, Cmd+I nel codice, messaggio del commit, correzioni dai Problemi e dal terminale.

**Fatto il 25 novembre:** il pannello di Claude rifatto, come l'estensione ufficiale: accesso e uscita dal pannello (abbonamento, Console o chiave API nel portachiavi), modello, ragionamento, fast mode e modo, il menu `/` con comandi e skill, `@` per file e cartelle del progetto, le chat che si continuano nel terminale e tornano aggiornate. Il dettaglio è in `HANDOFF.md`.

| Cosa | Chi ce l'ha | Peso |
|---|---|---|
| ~~**Revisione delle modifiche della sessione**~~ fatta con la 0.1.3: i commenti sulle righe raccolti e mandati a Claude in un colpo solo | Warp (anche per Claude Code), Xcode 27, Claude Code in VS Code | — |
| **Sessioni parallele:** più chat di Claude che lavorano insieme, ognuna in un worktree suo, con un elenco che dice chi ha finito e chi aspetta una risposta | Cursor 3, VS Code 1.140–1.146 («Inbox»), Zed 1.0 | grande |
| ~~**Il piano come documento**~~ fatto con la 0.1.3 (le note si scrivono nel piano stesso) | Claude Code in VS Code, Xcode 27 | — |
| ~~**Tornare indietro**~~ fatto con la 0.1.3: il codice riportato a un messaggio, e la chat divisa da una risposta | Claude Code in VS Code | — |
| **Le chat di Claude in una scheda** invece che solo nel pannello di destra, da affiancare al codice | Xcode 27, VS Code 1.140 | medio |
| **Anteprima nel browser integrato:** la scheda con la pagina del progetto e «Mostra a Claude» sono fatte con la 0.1.3; resta Claude che la usa da solo (clic, moduli) | VS Code 1.147 («Browser Use»), Cursor 3.7 | grande |
| **Cercare dentro il testo delle conversazioni passate** (oggi si cerca solo nei titoli delle chat) | Cursor 3.11 | medio |
| **Suggerimenti col Tab** (la riga successiva prevista mentre scrivi) 🔸 — serve decidere chi li calcola: un modello locale o un servizio | Zed (Zeta), Cursor, Copilot | grande |
| **Automazioni a orario** (un controllo ogni mattina, una revisione ogni sera) | VS Code 1.145, Cursor 3.8 | grande, più avanti |

## Scrivere il codice

| Cosa | Chi ce l'ha | Peso |
|---|---|---|
| **Colori dal language server** (semantic tokens): variabili, tipi e macro colorati per quello che sono, non solo per la sintassi | VS Code, Zed | medio |
| **Tipi e nomi dei parametri dentro la riga**, non in fondo: vuole una modifica all'editor di gpui-component (`vendor/gpui-base`) | VS Code, Zed, IntelliJ | grande |
| **Riferimenti, Problemi e rinomine in una scheda modificabile**, come già i risultati della ricerca (i «multibuffer» di Zed) | Zed | medio |
| **Modalità Vim** 🔸 | Zed (anche Helix), VS Code con estensione | grande |
| **Tag HTML rinominati in coppia** (linked editing) e **quadratini dei colori CSS** col selettore | VS Code, Zed | piccolo |
| **Mermaid nell'anteprima Markdown**, con zoom | VS Code 1.146, Zed 1.16 | medio |
| **CSV, TSV e JSONL come tabella** | Zed 1.17 e 1.23 | medio |
| **Tabelle Markdown da modificare cella per cella** | VS Code 1.147 | medio |
| **Controllo ortografico** nei commenti e nei Markdown, solo in locale | VS Code con Code Spell Checker, Zed con Harper | piccolo |

## Git

| Cosa | Chi ce l'ha | Peso |
|---|---|---|
| **Editor dei conflitti**: la vista a tre colonne, con la base e il risultato, e «Accetta la corrente», «Accetta quella in arrivo», «Entrambe» anche sopra i conflitti nell'editor normale (la scheda con un pulsante per conflitto c'è già dalla fase 5.3) | VS Code, IntelliJ | medio |
| **Worktree**: crearli, aprirli e chiuderli dalla vista Git, copiando le impostazioni del progetto e collegando le cartelle ignorate (`node_modules`, `target`) | VS Code 1.140, IntelliJ 2026.3, Zed | medio |
| ~~**«Cosa cambia rispetto a main»**~~ fatto con la 0.1.3 | Zed 1.29 | — |
| **Rebase interattivo** con le righe da spostare, unire o togliere | IntelliJ, GitLens | medio |
| **Pull request: i limiti di oggi**: segnare una discussione come risolta, i commenti sulle righe tolte, le immagini nei testi | GitHub, VS Code | medio |
| ~~**Trascinare un file dalla vista Git nel terminale o nell'editor**~~ fatto con la 0.1.3, anche nella chat di Claude | Zed 1.28 | — |

## Debug e test

| Cosa | Chi ce l'ha | Peso |
|---|---|---|
| **Il valore di una variabile passandoci sopra**, con l'albero apribile | VS Code, Zed 1.26 | medio |
| **Punti d'interruzione su funzioni e su dati** | VS Code, Xcode | medio |
| **Provare davvero il debug di Node e TypeScript** (oggi non provato su questo Mac) | — | piccolo |
| **Copertura dei test nel margine**: le righe toccate dai test in verde, le altre in rosso (`cargo llvm-cov`, Vitest, pytest-cov) | IntelliJ 2026.3, VS Code | medio |
| ~~**Correggere i test di un crate in una sottocartella senza workspace**~~ fatto con la 0.1.3 | — | — |
| **Valori colorati per tipo** nelle variabili del debugger | Zed 1.24 | piccolo |

## Terminale

| Cosa | Chi ce l'ha | Peso |
|---|---|---|
| **Terminale in una finestra sua** | richiesta tra le più votate di VS Code | medio (con il punto 10) |
| **Comprimere l'output di un comando** e copiarlo intero con un clic, sfruttando l'integrazione della shell che Keel ha già | Warp (blocchi) | medio |
| ~~**Ingrandire il testo con la rotella** tenendo Cmd~~ fatto con la 0.1.3 | Zed 1.28 | — |

## Interfaccia e Mac

| Cosa | Chi ce l'ha | Peso |
|---|---|---|
| **VoiceOver**: GPUI parla con VoiceOver attraverso AccessKit, ma Keel non è mai stato provato così, e i tanti pulsanti con la sola icona (barra, pannelli, schede) non hanno un nome da leggere | VS Code, Xcode | grande |
| **Lo stile personale (`style.css`)**, già proposto in `HANDOFF.md` 🔸 | Firefox, Zen | grande |
| **Barra degli strumenti personalizzabile** | Xcode 27 | medio |
| **Un tema per progetto**, per riconoscere le finestre (da verificare: forse basta già `workbench.colorTheme` nelle impostazioni del progetto) | Xcode 27, VS Code con Peacock | piccolo |
| **Profili** (impostazioni ed estensioni diverse per lavoro e per progetti personali) | VS Code | medio |
| **Impostazioni sincronizzate fra più Mac** con una cartella di iCloud Drive, senza account nuovi 🔸 | VS Code (con account) | medio |
| ~~**Comando `keel` nel Terminale**~~ fatto il 25 novembre: da «Prepara Keel» o dalla palette, e `keel .` apre Keel come app a sé | VS Code, Zed | — |

## Linguaggi

| Cosa | Chi ce l'ha | Peso |
|---|---|---|
| **Go** con `gopls`, colori e debug con Delve | tutti | medio |
| **Swift** con `sourcekit-lsp` di Xcode (già sul Mac) e debug con lldb-dap, che Keel usa già | Xcode, VS Code, Zed | medio |
| **PHP, Ruby, Kotlin, Vue, Svelte, Lua, Zig**: server e colori, uno alla volta secondo chi li chiede | VS Code, Zed | piccolo o medio ciascuno |
| **Un language server qualsiasi da `settings.json`**, per chi ne vuole uno che Keel non conosce | IntelliJ 2026.3 | piccolo |

## Pubblicazione e fiducia

| Cosa | Perché | Peso |
|---|---|---|
| ~~**Firma e notarizzazione di Apple**~~ | Non si fa, per scelta dell'autore. Il sito può spiegare l'avviso di macOS a chi scarica il DMG, e consigliare lo script di installazione | — |
| ~~**Scegliere la licenza**~~ | Decisa il 25 novembre: `LICENSE` (in italiano `LICENZA.it.md`), codice da leggere e basta, app gratis anche per lavoro | — |
| **Segnalare un crash, solo se lo vuoi**: alla riapertura dopo un crash, un pulsante che prepara la segnalazione con il registro, da mandare tu | Keel tiene già i registri dei crash | piccolo |
| **Le FAQ del sito su cosa Keel manda in rete**: mancano il fetch automatico di Git e Claude (KeelSite) | Correttezza | piccolo |

## Prestazioni, da misurare prima di promettere

- **Memoria grafica e CPU da fermo**: corrette il 24 novembre (vedi `HANDOFF.md`); il confronto con VS Code e Cursor del README e del sito è stato rimisurato il 27 novembre.
- **File grandi**: Keel chiede prima di aprirli, ma va misurato come si muove con un file di 50–100 MB e con un progetto di 100.000 file.
- **Tempo di avvio** con 20 schede e due progetti (oggi la prima schermata arriva in 330 ms con una finestra vuota).
- **Il benvenuto**: misurato al 22% di un core mentre si muove; si può abbassare disegnando le onde con colonne più larghe.

---

## Cosa ho lasciato fuori, e perché

- **Collaborazione in tempo reale** (Zed): utile a pochi, e vuole un servizio da gestire.
- **Sviluppo remoto via SSH e container** (Zed, VS Code, IntelliJ): è la cosa più grande di tutte. Vuole un pezzo di Keel che gira sull'altro computer. Da riconsiderare se servirà davvero, per esempio per il mini PC.
- **Notebook Jupyter**: già nelle fasi opzionali di `HANDOFF.md`.
- **Windows e Linux**: già nelle fasi opzionali.

---

## Fonti

- [VS Code 1.147](https://code.visualstudio.com/updates/v1_147), [1.146](https://code.visualstudio.com/updates/v1_146), [1.145](https://code.visualstudio.com/updates/v1_145) e [1.140](https://code.visualstudio.com/updates/v1_140): note di rilascio ufficiali.
- [Roadmap di VS Code](https://www.github.com/microsoft/vscode/wiki/Roadmap): le richieste più votate (pannelli e terminale staccabili).
- [Zed, rilasci stabili](https://zed.dev/releases/stable) e [funzioni](https://zed.dev/features).
- [Cursor 3.0](https://cursor.com/changelog/3-0) e [3.1](https://cursor.com/changelog/3-1); per le versioni successive [questo riassunto di Cursor 3.11](https://www.digitalapplied.com/blog/cursor-3-11-side-chats-agent-transcript-search-2026), non ufficiale.
- [What's new in Xcode 27, WWDC26](https://developer.apple.com/videos/play/wwdc2026/258/).
- [What's New in IntelliJ IDEA 2026.3](https://www.jetbrains.com/idea/whatsnew/).
- [Warp: revisione del codice degli agenti](https://www.warp.dev/blog/agents-3-full-terminal-use-plan-code-review-integration) e [supporto agli agenti esterni](https://warp.dev/blog/universal-agent-support-level-up-coding-agent-warp).
- [Claude Code in VS Code, documentazione ufficiale](https://code.claude.com/docs/en/vs-code).
- [Sondaggio di Stack Overflow 2026, tecnologie](https://survey.stackoverflow.co/2026/technology/data) e [l'articolo di Visual Studio Magazine](https://visualstudiomagazine.com/articles/2026/10/06/stack-overflow-2026-survey-vs-code-leads-visual-studio-takes-third.aspx); per Claude Code fra gli agenti, [Windows Latest](https://www.windowslatest.com/2026/11/23/the-thing-that-hit-us-satya-nadella-admits-vs-code-faded-in-six-months-after-a-decade-on-top-because-of-claude-code-and-some-developers-say-theyve-stopped-opening-it/), una fonte secondaria.
