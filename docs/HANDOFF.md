# Keel — passaggio di consegne

*Scritto il 18 novembre 2026, aggiornato il 25 novembre con il pannello di Claude rifatto, l'import da VS Code, «Prepara Keel», il comando `keel` e Keel che da fermo non consuma più, usciti con la 0.1.2; il 26 novembre con i documenti allineati e la proposta della 0.1.3 ([`PIANO-0.1.3.md`](PIANO-0.1.3.md)); il 27 novembre con la 0.1.3 fatta (sottofasi 1–5) e il confronto rimisurato, in attesa dell'uscita. Cosa cambia in ogni versione è in [`CHANGELOG.it.md`](CHANGELOG.it.md).*

Questo documento serve a chi riprende il lavoro su Keel, persona o assistente, senza aver seguito la storia. Spiega:
- che cos'è il progetto e perché esiste;
- com'è fatto;
- cosa è stato fatto finora;
- cosa resta da fare.

Il dettaglio di ogni fase, con le decisioni e le prove, è in [`PIANO.md`](PIANO.md).

---

## 1. Cos'è Keel e perché esiste

Keel è un **IDE personale per macOS scritto in Rust**. L'idea è avere un editor:
- **veloce e leggero:** con lo stesso progetto e lo stesso file Keel occupa 180 MB, VS Code 748 e Cursor 1.344 (la colonna Memoria di Monitoraggio Attività, 27 novembre; il confronto è al punto 3);
- **bello:** stile Apple, disposizione come Cursor, tema predefinito blu mezzanotte;
- **familiare:** stesse scorciatoie e stessi nomi dei comandi di VS Code, `settings.json` e `keybindings.json` compatibili;
- **estendibile:** le estensioni di VS Code funzionano dove possibile, e quelle dell'autore sempre. La prova del nove è **VSAuto Ghost**, la sua estensione di «fake coding».

Nasce dalla curiosità per le app native in Rust e dal desiderio di un editor tutto suo. Cursor resta installato come riserva. È pubblicato dal 23 novembre 2026: si scarica da https://keel.mezzalab.eu, la pagina di presentazione che sta in una repo sua (`KeelSite`), e si aggiorna da solo; la firma di Apple non si fa, per scelta dell'autore (punto 4, «Punti aperti»). Il codice è pubblico su GitHub dal 24 novembre.

### Perché è fatto così

- **Niente fork di VS Code:** sarebbe Electron, con la stessa RAM di Cursor.
- **Niente fork di Zed:** è enorme e sotto licenza GPL.
- **Keel si costruisce su GPUI**, il motore grafico di Zed che disegna con la scheda video (Metal), e su **gpui-component**. Entrambi hanno licenza Apache. Dall'editor di Zed non si copia codice.
- **Le estensioni di VS Code passano da tre «prese»:**
  1. un lettore di pacchetti VSIX per temi, icone, snippet, grammatiche e impostazioni;
  2. gli stessi language server che usa VS Code;
  3. un ponte `vscode` scritto in JavaScript, che gira nel motore V8 (`deno_core`) dentro Keel, senza Node completo.

  I pacchetti arrivano da Open VSX, perché il marketplace di Microsoft è vietato agli altri editor.

---

## 2. Com'è fatto

### Il repository

| Percorso | Contenuto |
|---|---|
| `crates/keel` | l'app: finestre, interfaccia, tutte le viste (~93.000 righe) |
| `crates/editor` | aggiunte all'editor: decorazioni, testo fantasma, comandi sulle righe |
| `crates/extensions` | lettore VSIX, Open VSX, host V8 con il ponte `vscode` (in `src/js`), impostazioni delle estensioni |
| `crates/lsp` | client dei language server, installazione dei server, linguaggi |
| `crates/language` | grammatiche tree-sitter e registro dei linguaggi |
| `crates/files` | albero dei file, ricerca nei file, fuzzy matching, cestino |
| `crates/git` | stato, differenze, commit, grafico |
| `crates/dap` | client dei debugger (protocollo DAP di VS Code), e dove si trovano lldb-dap, debugpy e js-debug |
| `crates/terminal` | terminali su `alacritty_terminal`, porte aperte |
| `crates/settings` | impostazioni utente e di progetto (JSONC, commenti conservati) |
| `crates/workbench` | logica senza interfaccia condivisa col ponte: scorciatoie, clausole «when», posizioni, URI |
| `crates/ai` | Claude Code: protocollo IDE e pannello agente ACP |
| `crates/pdf` | anteprima PDF con PDFKit |
| `crates/web` | pagine web in una scheda: il WebKit di macOS (`WKWebView`) sopra la finestra, come in Sail |
| `crates/update` | aggiornamenti di Keel: elenco delle versioni, download controllato (peso, SHA-256, firma Ed25519), sostituzione dell'app dopo la chiusura |
| `vendor/gpui-base` | copia dell'editor di gpui-component, con le correzioni che servivano (pieghe, più cursori) |
| `crates/keel/icons/material` | le icone di Material Icon Theme 5.40.0 (MIT) che Keel usa di serie, con l'elenco e la licenza; si aggiornano con `scripts/update-material-icons.sh` |
| `fixtures/` | file e pacchetti di estensioni per i test |
| `scripts/` | `bundle-macos.sh` (crea `Keel.app`), `make-dmg.sh` (crea il DMG), `measure-memory.sh`, `release.sh` e `install.sh` (pubblicazione), `update-material-icons.sh` (le icone da una versione nuova di Material Icon Theme), `third-party-licenses.py` (gli avvisi delle licenze, con i testi mancanti in `licenses/`) |
| `assets/` | il marchio e le immagini del README (`readme/`: testata e confronto sono SVG animati, il resto sono finestre vere disegnate da Keel). Il video di 40 secondi in cima ai due README non sta qui: è caricato su GitHub (un link `user-attachments`, l'unico modo perché GitHub lo mostri con il player) e si rifà con `film/render.sh` della repo `KeelSite` |
| `README.md` | la presentazione per chi arriva sulla repo, in inglese; in italiano è `docs/README.it.md`. Vanno aggiornati insieme |
| `LICENSE` | la licenza di Keel, in inglese; in italiano è `docs/LICENZA.it.md`. Codice da leggere e basta, app gratis da usare anche per lavoro (punto 4, «Punti aperti») |
| `CHANGELOG.md` | cosa cambia in ogni versione, in inglese; in italiano è `docs/CHANGELOG.it.md`. A ogni versione si aggiungono tutti e due, e le note del rilascio sono il loro riassunto |
| `docs/` | questo documento, il piano, i dettagli da aggiungere (`DETTAGLI.md`) e le bozze grafiche, in italiano |

L'interfaccia sta quasi tutta in `crates/keel/src/workbench_window/`, un file per vista:
- `shell.rs`: disposizione e schede;
- `settings_view.rs`: pagina Impostazioni;
- `extensions_view.rs`: vista Estensioni;
- `source_control.rs` e `scm_graph.rs`: Controllo versione e grafico;
- `search_view.rs`: ricerca;
- `terminal_view.rs`: terminale;
- `tabs.rs`: gruppi di editor, schede trascinate, divisioni in ogni direzione;
- `claude_inline.rs`: Claude nell'editor (Cmd+I) e il messaggio del commit;
- `agent_panel.rs`, `agent_view.rs`, `agent_session.rs`, `agent_cards.rs`, `agent_chats.rs`: il pannello di Claude (la conversazione con l'agente, il disegno con i menu `/` e `@`, account e limiti, le schede di modello, modo e consumo, le chat per progetto); il protocollo è in `crates/ai` (`acp.rs`, `session_meta.rs`, `history.rs`);
- `agent_review.rs`: le modifiche di Claude file per file, con Accetta, Rifiuta e i commenti sulle righe;
- `web_preview.rs`: la pagina web in una scheda, sopra `crates/web`;
- `contributions.rs` e `project_icons.rs`: le icone di file e cartelle (quelle di Material di serie) e l'immagine di ogni progetto;
- `testing.rs`: test dal margine e vista Test;
- `debug.rs`, `debug_config.rs`, `debug_view.rs`, `breakpoints.rs`: debugger, sopra `crates/dap`;
- `tasks.rs`, `task_config.rs`, `problem_matchers.rs`: attività di `tasks.json`, con gli errori nei Problemi;
- `github.rs` e `pull_requests.rs`: pull request, attraverso il programma `gh`;
- `diff_edit.rs`, `search_results.rs` e `file_pieces.rs`: differenze e risultati della ricerca modificabili, con la scrittura nei file che non sovrascrive niente di cambiato;
- `blame_card.rs`, `line_hints.rs`, `rulers.rs`: scheda del commit, riferimenti e tipi in fondo alla riga, righelli;
- e così via.

I testi dell'interfaccia sono in `crates/keel/src/i18n.rs`, in italiano e in inglese. I temi Mezzanotte, Chiaro e Nero sono in `crates/keel/themes/keel.json`.

### Versioni fissate

- Rust 1.100.0 (`rust-toolchain.toml`);
- `gpui-pre` 0.3.12;
- `gpui-component` 0.9.1;
- `deno_core` 0.413.0;
- `alacritty_terminal` 0.26.

Le versioni si aggiornano solo di proposito, e con i test.

### Dove tiene i dati

`~/Library/Application Support/com.manuelmezzanotte.keel` contiene:
- `settings.json` e `keybindings.json`;
- estensioni, sessioni, progetti recenti, copie di emergenza e registri dei crash;
- `welcome-seen`, scritto quando finisce il benvenuto del primo avvio: finché manca, e manca ogni altra traccia di Keel, la prima finestra si apre col benvenuto. `KEEL_WELCOME=1` lo mostra comunque, per provarlo.

Per le prove si usa sempre una cartella a parte, con `KEEL_DATA_DIR`, mai quella vera.

### Comandi utili

```bash
cargo run -p keel                              # avvio in sviluppo
cargo test --workspace                         # tutti i test
cargo clippy --workspace --all-targets         # controlli
scripts/make-dmg.sh                            # Keel.app + target/release/Keel-<versione>.dmg
scripts/snapshots.sh <cartella>                # la finestra disegnata fuori dallo schermo, in PNG
scripts/third-party-licenses.py                # gli avvisi delle licenze, dopo aver cambiato le dipendenze
```

---

## 3. Cosa è stato fatto

Tutte le tredici fasi del piano sono fatte, dalla 0 alla 12, tra il 14 e il 18 novembre 2026:

| Fase | Contenuto |
|---|---|
| 0 | Prototipo: editor, palette dei comandi, host V8, VSAuto Ghost funzionante |
| 1 | Impostazioni, italiano e inglese, comandi e scorciatoie, temi, disposizione (bozza D), menu, marchio (barchetta di carta) |
| 2 | Progetti e finestre, Esplora risorse, schede e split, Cmd+P, salvataggi sicuri, cambi dal disco, sessioni |
| 3 | Ricerca e sostituzione nei file |
| 4 | Terminali con split, pannello Problemi/Output/Terminale/Porte, Claude Code a destra, script npm |
| 5 | Git: modifiche, stage, commit, sincronizzazione |
| 6 | Language server per il web e per Rust |
| 7 | Anteprima Markdown e PDF |
| 8 | Git Graph e storico |
| 9 | Estensioni: VSIX, Open VSX, ponte `vscode`, vista Estensioni, «Importa da Cursor» |
| 10 | Claude Code: protocollo IDE e pannello agente |
| 11 | Altri linguaggi: Python, Java/Spring, C/C++, C#, Bash, Docker, XML, SQL… |
| 12 | Rifinitura: crash e copie di emergenza, pieghe, più cursori, barra di ricerca con regex, minimappa, sticky scroll, pagine Impostazioni e Scorciatoie, contrasto AA, prestazioni |

Dopo le prime prove dell'app vera (18 novembre) sono arrivati altri giri di ritocchi:
- Mezzanotte più chiaro;
- barra laterale ridimensionabile, con le righe che seguono la sua larghezza;
- Controllo versione rifatto, con il grafico dentro;
- pagina Cerca più ricca;
- DMG e icona dell'app;
- per le estensioni:
  - pagine di impostazioni, con i cambi che arrivano subito all'estensione;
  - proposta dei temi appena installate;
  - icone del tema di icone in ogni lista;
  - etichette «Funziona in parte» e «Non funziona in Keel»;
- correzione dei controlli della pagina Impostazioni, che non salvavano (`8fbd01c`);
- **nuovo aspetto** (sera del 18 novembre): due stili della finestra, «Pannelli sospesi» e «Piatto», scelti dalla nuova pagina Impostazioni → Interfaccia (`keel.interfaceStyle`); animazioni di pannelli, selettore e sezioni (`keel.animations`); Controllo versione con messaggio più grande, pulsante Commit largo e grafico ridimensionabile; Cerca, Estensioni e benvenuto senza spazi vuoti; pulsanti più grandi; Cmd+P e tavolozza su una superficie sola. Il dettaglio è in `PIANO.md`, «Nuovo aspetto». Poi l'icona nel formato di Icon Composer, senza il riquadro nero di macOS, e il pannello di Claude che si apre sulla chat con le chat di Claude Code del progetto («Icona nuova e pannello di Claude»).
- **i dettagli** (19–20 novembre): 143 piccoli dettagli raccolti da VS Code, Cursor, Zed, JetBrains, Xcode e dal Mac, fatti in nove giri, dal commit `db6bbbe` al `66e6bfb`. Tra gli altri: avvisi e finestre d'errore di git, differenze rifatte, salvataggio automatico, cronologia locale, segnalibri, cronologia degli appunti, terminale con l'integrazione della shell (pallini, salti fra i comandi, correzioni rapide, testo che torna dopo il riavvio), menu Finestra e Aiuto, barra di stato cliccabile con codifiche e fine riga, Quick Look, file trascinati fuori da Keel. Voce per voce, con cosa resta e come provarlo, in [`DETTAGLI.md`](DETTAGLI.md).
- **le funzioni nuove** (21–22 novembre), dal commit `50e24d1` al `2e4b0a5`, più i documenti. Sono su GitHub, su `develop`. La versione con tutte queste è installata in /Applications dal 22 novembre:
  - una scheda trascinata su un bordo divide l'editor, anche sopra e sotto;
  - Claude nell'editor (Cmd+I) e il messaggio del commit scritto da Claude;
  - i test dal margine, con la vista Test;
  - la scheda del commit sull'autore della riga, i righelli, i riferimenti e i tipi in fondo alla riga;
  - la colonna di destra delle differenze e i risultati della ricerca modificabili;
  - le attività di `tasks.json`, con gli errori nei Problemi;
  - il debugger, con la Debug Console;
  - le pull request di GitHub, con i controlli e i commenti.

  Con queste i dettagli di `DETTAGLI.md` sono 131 fatti, 9 a metà e 3 non ancora.

  Come provarle, cosa è stato provato davvero e i limiti di ognuna sono in [`DETTAGLI.md`](DETTAGLI.md), «Le funzioni nuove».
- **la pagina di presentazione** (22 novembre), in una repo sua, `KeelSite` (privata su GitHub, sul Mac accanto a Keel): React, Vite, Tailwind e shadcn/ui, in italiano e in inglese. Dal cielo si scende nel mare, che copre una copia di Keel in HTML, navigabile e nei tre temi; poi il confronto misurato con VS Code e Cursor, le funzioni, i linguaggi, le estensioni e le domande. I numeri e come sono stati misurati stanno nel suo `src/site.ts`. È online su https://keel.mezzalab.eu dal 23 novembre, servita dal mini PC: ogni push su `main` di `KeelSite` la pubblica da solo. Oltre alla presentazione ha la pagina di download (legge `/updates.json`), quella della privacy e, dal 25 novembre, quella delle novità (`/changelog/`).

- **l'interfaccia rifatta e il benvenuto** (24 novembre):
  - **Impostazioni** disegnate da Keel (`settings_page.rs`) invece che dal componente di gpui-component: le pagine con la loro icona e il numero di impostazioni cambiate, una ricerca in italiano su tutte le pagine (nome, descrizione e chiave, ogni parola conta), le impostazioni in riquadri divisi per gruppo, una barra a sinistra su quelle cambiate e il pulsante per tornare al valore di Keel. I temi preferiti si scelgono da un elenco. Raggio e spazio dei pannelli e «Schede su più righe», che non si vedevano più, sono tornati nella pagina Interfaccia; un test controlla che ogni impostazione del catalogo abbia il suo posto;
  - **scorciatoie da tastiera** con titolo, ricerca, tasti disegnati come tastini, «Tua» in evidenza e i pulsanti solo sulla riga sotto il puntatore;
  - **pagina di benvenuto** con il marchio, «L'IDE per Mac scritto in Rust», le azioni e i progetti recenti in due colonne e un mare calmo in fondo;
  - **il benvenuto del primo avvio** (`intro.rs`): sopra la prima finestra un cielo (di notte con un tema scuro, di giorno con uno chiaro), il mare che sale, la barca di Keel che entra e dondola, il nome, la frase e «Sali a bordo». Con il pulsante, Invio, Esc o lo spazio il mare sale su tutta la finestra e si schiarisce sull'IDE. È disegnato solo con rettangoli (le onde sono colonne sottili) e con la barca in nove piccoli SVG (`assets/intro/`): niente tracciati, che terrebbero in memoria immagini grandi come la finestra. Si ridisegna sessanta volte al secondo solo con la finestra in primo piano (circa il 22% di un core mentre si muove, misurato sulla versione release), e si ferma quando è dietro le altre. Con Riduci movimento o `keel.animations` spento resta fermo e il pulsante entra subito. Gli avvisi che arrivano intanto aspettano la fine. Aiuto → «Rivedi il benvenuto» (comando `keel.showWelcome`) lo fa rivedere;
  - `scripts/snapshots.sh` fotografa anche le pagine nuove (`settings-editor`, `settings-updates`, `settings-search`, `keybindings`) e il benvenuto fermo in sette momenti (`intro-mezzanotte-0`…`out-3`, lo stesso di giorno); con `KEEL_SNAPSHOTS_FILM=1` anche tutto il benvenuto, quindici fotogrammi al secondo, per farne un video con ffmpeg.

- **i primi minuti** (25 novembre, usciti con la 0.1.2):
  - **Importa da VS Code**, oltre che da Cursor, con lo stesso codice (`other_editor.rs`, `import_settings.rs`, `import_extensions.rs`): impostazioni, scorciatoie, frammenti (`snippets/`, nuovi anche per Cursor) e progetti recenti dopo una domanda, poi le estensioni nella vista Estensioni. Niente di Keel viene sovrascritto. I progetti recenti si leggono dal database dell'editor, anche mentre è aperto (`immutable=1`: prima con Cursor aperto non si leggevano), oppure dal menu «Apri recenti» che VS Code tiene in `storage.json`. Comandi: «Preferenze: Importa impostazioni… da VS Code», «Estensioni: Importa da VS Code»;
  - **«Prepara Keel»**, la scheda che segue il benvenuto del primo avvio (`setup_sheet.rs`): il tema, «Importa» per ogni editor trovato sul Mac, il comando `keel`. Esc o «Inizia a lavorare» la chiudono;
  - **il comando `keel` nel Terminale**: un collegamento in `/usr/local/bin` al programma di Keel; se la cartella è del sistema, macOS chiede la password (la scrive la persona, nella finestra di macOS). Si installa anche dalla palette, «Keel: Installa il comando keel nel Terminale». Lanciato da un terminale con Keel chiuso, `keel .` apre Keel come app a sé e restituisce subito il prompt, come `code .` (`KEEL_FOREGROUND=1` lo tiene nel terminale). Provato dal vivo: prompt in 0,7 s, Keel figlio di macOS e non del Terminale.

- **il pannello di Claude rifatto** (25 novembre, uscito con la 0.1.2). Il pannello parla con Claude Code attraverso l'adattatore ACP (`claude-agent-acp`), che dice già tutto quello che serve: account, modi d'accesso, modelli, comandi e skill, chat. Keel ora lo usa:
  - **accesso e uscita dal pannello**: se Claude Code non ha un login, al posto della chat compare «Accedi a Claude» con «Abbonamento Claude» (Pro, Max, Team, Enterprise) e «Console Anthropic» (a consumo). Il login è quello di Claude Code (`claude auth login`), lanciato in un terminale del pannello in basso, dove si apre il browser; Keel chiede ogni 2 secondi se è arrivato (per 10 minuti al massimo) e poi apre la chat. Il menu dell'account nell'intestazione (l'iniziale dell'email) mostra il piano e l'email, ed ha «Esci»;
  - **chiave API propria**: «Usa una chiave API» la tiene nel portachiavi di macOS (servizio «Keel · Anthropic API key», mai in un file) e riavvia Claude con `ANTHROPIC_API_KEY`; «Dimentica la chiave API» torna all'account. Si accetta solo ciò che sembra una chiave (`sk-ant-…`);
  - **modello, ragionamento e fast mode** in una scheda sopra il messaggio («Opus 5.5 · Alto»): i cinque modelli principali e «Altri modelli» per i vecchi, il ragionamento come un cursore da trascinare (una barra arrotondata che si riempie fino alla manopola, un punto e un nome per ogni livello, una riga che dice cosa fa il livello scelto; anche un clic su un punto o su un nome lo sceglie) e «Auto» come interruttore accanto al titolo, la modalità veloce con un interruttore. **Il modo** in un'altra scheda, con una riga di spiegazione per ognuno (Manuale, Accetta modifiche, Piano, Automatico, Senza permessi in arancione). Sono le opzioni della sessione (`configOptions`): i valori sono quelli di Claude Code, i nomi in italiano li mette Keel (`agent_cards.rs`). In un pannello stretto (sotto i 400 punti) il modo mostra solo il simbolo;
  - **il contesto e i limiti**: un anello nel campo di scrittura dice quanto è pieno il contesto della chat (da `usage_update`). Cliccandolo si apre la scheda con il contesto («313k di 1M token»), il piano e i limiti delle 5 ore e della settimana con quando si azzerano, come nell'app Claude. I limiti vengono dal `/usage` di Claude Code, lanciato in una sessione a parte (cartella `claude-usage` nei dati di Keel) e cancellata subito: è un comando locale, non consuma token (provato: 2,4 s, 0 token). Ogni turno aggiorna il limite che riporta (`_claude/rateLimit`). Con una chiave API la scheda mostra il costo della chat e rimanda alla Console per il saldo: l'API di Anthropic non lo dà;
  - **ragionamento e comandi**: il ragionamento è una riga con le sue prime parole (e una rotellina mentre Claude pensa ancora); cliccandola si apre il testo intero, leggibile (non più in corsivo, che con il carattere dell'interfaccia poteva sparire). Una riga aperta in una chat scorsa restava vuota: la scheda ritagliava il suo contenuto, e dentro una lista scorsa GPUI lo tagliava tutto; ora non ritaglia più. Le foto possono rigiocare una chat vera (`KEEL_SNAPSHOTS_REPLAY=<file json con gli aggiornamenti dell'adattatore>`, foto `claude-replay` e `claude-replay-open`). Un comando su più righe mostra la prima nella riga e tutto il comando nella scheda aperta, invece di sovrapporsi alle righe sotto; i messaggi vuoti di una chat riaperta non lasciano più buchi. Foto: `claude-thinking`, `claude-thinking-open`;
  - **eliminare una chat**: il cestino compare passando sulla riga; il primo clic chiede «Eliminarla per sempre?», il secondo la cancella in Claude Code (`session/delete`, tutti i suoi file). Una chat cancellata non torna anche se una lista chiesta prima arriva dopo;
  - **`/` all'inizio del messaggio**: il menu con i comandi di Claude Code e le **skill dell'utente** (arrivano con `available_commands_update`), filtrati mentre si scrive; frecce, Invio o Tab per usarne uno, Esc per chiudere;
  - **`@` ovunque nel messaggio**: la selezione, i file aperti, e scrivendo i file e le cartelle del progetto (ricerca fuzzy, come Cmd+P); quello scelto va insieme al messaggio;
  - **le chat dell'app Claude**: Keel legge (senza mai scriverci) le chat della scheda Code dell'app Claude (`~/Library/Application Support/Claude/claude-code-sessions/*/*/local_*.json`): quelle della cartella del progetto compaiono con il loro titolo e il segno di Claude, al posto delle sessioni di Claude Code su cui girano (una chat dell'app che ha riassunto il contesto è fatta di più sessioni: prima Keel le mostrava tutte, con lo stesso titolo). Aprendone una Keel continua la sua sessione attuale; se è stata usata negli ultimi 10 minuti avvisa di scrivere in un posto alla volta. Una chat dell'app si elimina dall'app (sparisce anche da Keel). Come nella barra laterale dell'app, Keel mostra anche le chat degli **altri progetti**: le chat sono in una scheda per progetto (`agent_chats.rs`), prima quella del progetto aperto («questo progetto», cartella arancione), con il nome, quante chat ha e una freccia per chiuderla; nella pagina iniziale le ultime di ognuno con «Mostra tutte», in «Tutte le chat» tutte, quelle del progetto aperto divise per giorno. Le chat dell'app hanno il segno di Claude accanto al titolo e niente cestino. Aprendo una chat di un altro progetto, Keel apre quel progetto nella sua finestra (o la porta davanti) e continua lì la chat, perché Claude deve lavorare nella cartella della chat. Sono solo le chat dell'account con cui l'app è collegata ora (da `config.json` dell'app Keel legge solo `lastKnownAccountUuid`), senza le archiviate e senza le cartelle temporanee dell'app (`scratch-workspaces`);
  - **chat in comune con Claude Code nel terminale**: le chat sono i file di Claude Code (`~/.claude/projects/…`). «Continua nel terminale» (menu dell'account) apre la chat nel terminale di destra con `claude --resume <id>` (o `/resume` se Claude Code è già aperto lì). Tornando alla chat, o tornando su Keel da un'altra app, Keel guarda se il file della chat è cambiato e la rilegge (`session/close` e `session/load`), con la lista delle chat;
  - **l'aspetto**: il segno di Claude (`assets/brand/claude-mark.svg`, nel suo arancione) al posto delle scintille, una pagina iniziale con «Cosa facciamo?», tre domande pronte e le chat recenti; i messaggi dell'utente in fumetti a destra, le risposte a tutta larghezza, gli strumenti come righe leggere (le modifiche aperte con le righe +/−), il piano con la barra d'avanzamento, i permessi in un riquadro arancione, un campo di scrittura con @, /, modo, modello e invio;
  - il codice: `agent_panel.rs` tiene la conversazione e il dialogo con l'agente, `agent_view.rs` la disegna (menu `/` e `@` compresi), `agent_session.rs` gestisce account, chiave, opzioni, limiti, eliminazione e sincronizzazione, `agent_cards.rs` disegna le schede di modo, modello e consumo; `crates/ai/src/session_meta.rs` legge i messaggi del protocollo. L'agente finto dei test (`fake_agent.rs`) fa anche account, logout, comandi e opzioni. Le foto: `KEEL_SNAPSHOTS_ONLY=claude scripts/snapshots.sh <cartella>` (`claude-home`, `-chats`, `-chat`, `-slash`, `-mention`, `-model`, `-mode`, `-usage`, `-tools`, `-login`);
  - provato: i test con l'agente finto (comandi, file, modello, logout, tasti dei menu) e la forma delle risposte dell'adattatore vero per l'account. **Da provare a mano**: una chat dell'app Claude continuata in Keel e poi riaperta nell'app (l'app potrebbe mostrare i messaggi di Keel solo dopo averla riaperta); login e logout veri (il logout esce anche da Claude Code nel terminale, perché il login è lo stesso), la chiave API, una chat continuata nel terminale e ripresa nel pannello.

- **la 0.1.3** (26 e 27 novembre, su `develop`, non ancora uscita), in cinque sottofasi:
  - *Claude:* le chat degli altri progetti si aprono nella finestra dove si lavora, con Claude nella loro cartella; Esc ferma Claude; un messaggio scritto mentre Claude lavora aspetta il turno; la chat aperta torna riaprendo la finestra; rinomina delle chat, copia delle risposte, «Aggiungi a Claude» dall'albero; il nome del modello leggibile; il Mac sveglio mentre Claude lavora; la dettatura (⌘D); il codice riportato a un messaggio e le chat divise da una risposta; il piano in una scheda, rimandato con le note; i commenti sulle righe delle modifiche di Claude, mandati insieme;
  - *le icone:* Material Icon Theme dentro Keel, di serie, e l'immagine di ogni progetto;
  - *l'anteprima web:* la pagina del progetto in una scheda, col WebKit di macOS (`crates/web`);
  - *Git e difetti:* cosa cambia il ramo rispetto a `main`, i file trascinati dalla vista Git, ⌘ con la rotella nel terminale, i test di un crate senza workspace, Cmd+P che apre dietro l'elenco, «Non mostrare più» sugli avvisi, il messaggio del commit in una finestra grande.

  Il dettaglio, quello che è andato diversamente e le prove a mano sono in [`PIANO-0.1.3.md`](PIANO-0.1.3.md), «Com'è andata».

Stato dei controlli all'ultimo commit:
- tutti i test passano: 607, più 28 da lanciare a mano; nell'app 343, più 6 (27 novembre);
- clippy non segnala niente;
- la prima schermata di Keel arriva in 330 ms, media di sei avvii;
- **memoria di Keel** (22 novembre, versione installata, M4 Pro, language server spenti), come la dà Monitoraggio Attività: 132 MB con una finestra vuota, 271 MB con un progetto e un file, 299 MB con 450 file e 20 schede, 326 MB con due progetti. I 130–150 MB scritti in `PIANO.md` sono la memoria residente (RSS), che non conta quella grafica: non vanno usati per i confronti;
- **confronto** (rimisurato il 27 novembre con la 0.1.3, stesso Mac, stesso progetto e stesso file Rust, profilo vuoto, nessuna estensione, media di tre avvii; il 22 novembre erano 265, 1.014 e 1.320 MB):

  | | Keel | VS Code 1.125 | Cursor 3.30 |
  |---|---|---|---|
  | Memoria di tutti i processi, dopo 40 s | 180 MB | 748 MB | 1.344 MB |
  | Avvio | 448 ms | 1.038 ms | 1.636 ms |
  | App su disco | 144 MB | 1.096 MB | 1.436 MB |
  | Processi | 1 | 8 | 15 |

  Per Keel l'avvio è fino alla finestra sullo schermo; per VS Code e Cursor è il tempo che dichiarano loro (`--prof-append-timers`), in avvii a parte perché quell'opzione li chiude appena scrive i tempi; la loro finestra vuota compare più o meno quando quella di Keel. Su disco VS Code e Cursor sono universali, Keel è solo per Apple Silicon. Come rifarlo: ogni avvio con una cartella sua (`KEEL_DATA_DIR`, `--user-data-dir` ed `--extensions-dir`); per VS Code e Cursor il percorso del profilo deve essere corto (il loro socket non supera 103 caratteri), per esempio dentro `target/`; la memoria è `footprint` (la `phys_footprint`) di tutti i processi dell'app, quelli figli e quelli che usano la sua cartella. Il lanciatore di VS Code e Cursor resta il processo principale; quello di Keel, aperto fuori dal Terminale, anche.

---

## 4. Cosa resta da fare

### La 0.1.3

Il piano è in [`PIANO-0.1.3.md`](PIANO-0.1.3.md), approvato il 26 novembre: sei sottofasi, dalle cose di Claude all'uscita. Le prime cinque sono fatte (26 e 27 novembre), con `CHANGELOG.md`, `docs/CHANGELOG.it.md`, i README e la pagina delle novità del sito già scritti. Resta l'uscita: `scripts/release.sh 0.1.3 <note>` con la VPN accesa (il codice di verifica lo scrive l'autore), poi il push di `develop` e del tag, e del sito, quando lo chiede l'autore.

### Subito: le prove a mano

Le fasi sono fatte, ma l'autore deve ancora provarle a mano. L'elenco di ogni fase è in `PIANO.md`, nei punti «Le tue prove a mano». I difetti trovati diventano ritocchi, come quelli del 18 novembre.

Delle funzioni nuove restano da provare davvero due cose, che qui non si potevano provare:
- **il debug di Node e TypeScript:** vuole `js-debug` di Microsoft, che su questo Mac non c'è. Keel lo scarica da GitHub (1,2 MB) solo quando glielo si chiede, e controlla che il file abbia l'impronta pubblicata;
- **le azioni che scrivono su GitHub:** commenti, approvazioni, unione e creazione di una pull request sono provate solo con un `gh` finto.

### Difetti trovati, da correggere

Trovati il 22 novembre misurando e fotografando Keel per la pagina di presentazione:
- **CPU da fermo e memoria grafica: corrette il 24 novembre**, uscite con la 0.1.2.
  - *La CPU* (7% di un core con un file aperto e la finestra davanti) veniva da un giro senza fine: le decorazioni dell'editor (parentesi colorate, la parola sotto il cursore, i segni nella barra di scorrimento) avvisano l'editor quando cambiano; l'editor avvisava Keel come se il cursore si fosse mosso, e dopo 160 ms Keel le ricalcolava e le rimetteva. Erano sei ridisegni al secondo senza fare niente. Ora si ricalcolano solo se cambiano testo, cursore o tema (`writing.rs`, `cursor_moved`), e decorazioni uguali non si rimettono. Un test lo controlla.
  - *La memoria grafica* (+125 MB) non era di Keel né dei tracciati: GPUI riserva le stesse cose con o senza file (misurato con una copia di GPUI che annotava ogni allocazione). È la memoria che macOS tiene per una finestra che si ridisegna spesso, e il cursore che lampeggia la ridisegna due volte al secondo. Ora il cursore smette di lampeggiare dopo 10 secondi senza tasti né clic e resta visibile, come in GNOME (`theme::caret_blink`); `"editor.cursorBlinking": "solid"` lo tiene sempre fermo. Dopo qualche secondo fermo macOS si riprende la memoria.
  - *Misure* (versione release, M4 Pro, progetto Keel con `layout.rs` aperto, finestra davanti): CPU da fermo dal 7,1% allo 0%; memoria da 272 a 157 MB, 15 secondi dopo l'ultimo tasto. Mentre si scrive i 120 MB tornano, perché la finestra si aggiorna: succede a ogni app che si ridisegna.
- **test di un crate in una sottocartella senza workspace: corretto il 27 novembre** (0.1.3): partono dalla cima del progetto con `--manifest-path`.

### Prossimo lavoro proposto: lo stile personale

**Non è da fare subito:** si parte solo col via dell'autore, dopo le prove.

L'idea è permettere a chi usa Keel di cambiarne l'aspetto con un suo file, come `userChrome.css` di Firefox e le mod di Zen. Vale solo per le cose visive, non per quelle essenziali.

- **Il file:** `style.css` nella cartella dei dati, accanto a `settings.json`, aperto con il comando «Apri il mio stile».
- **Il linguaggio:** CSS, perché è corto da scrivere e lo conosce chi lavora sul web. Le regole puntano alle parti di Keel che hanno un nome. Esempio:

  ```css
  .sidebar { floating: off; border-radius: 0; box-shadow: none; }
  .tab { height: 28px; }
  .tab.active { border-bottom: 2px solid #5b8cff; }
  .breadcrumbs { display: none; }
  .sidebar .git { background: #0b1020; }
  ```

- **Cosa raggiunge:** tutto quello che disegna Keel, e ogni finestra usa lo stesso stile:
  - finestra e barra del titolo;
  - barra laterale e le sue sezioni;
  - schede ed editor, compresi i colori della sintassi;
  - terminale, barra di stato, pagine Impostazioni ed Estensioni;
  - Cmd+P, palette, menu, avvisi, suggerimenti.
- **Cosa cambia:** colori, caratteri, dimensioni, spazi, angoli, bordi, ombre, trasparenze; parti fluttuanti o attaccate al bordo; posizione della barra laterale; pezzi non essenziali da nascondere.
- **Cosa resta fuori:**
  - ciò che disegna macOS: barra dei menu, pallini della finestra, finestre di sistema;
  - i comportamenti nuovi: nessun codice che esegue azioni;
  - l'interno di alcuni controlli di gpui-component, dove si arriva solo a colori, dimensioni e angoli.
  - Editor, Impostazioni e palette non si possono nascondere, così non ci si chiude fuori.
- **Il file dell'utente vince sempre.** L'ordine è: valori di Keel, poi tema colori, poi `style.css`.
  - A ogni avvio, anche dopo un aggiornamento, il file si applica prima di mostrare la finestra, quindi non c'è mai un attimo con l'aspetto di base.
  - Il file sta nella cartella dei dati, non dentro `Keel.app`: un DMG nuovo non lo tocca mai.
  - Una parte nuova ha l'aspetto di base finché l'utente non la cambia.
  - Se una parte viene rinominata o tolta, Keel lo avvisa nell'Output e non ignora la regola in silenzio. I nomi si tengono stabili: se serve rinominare, il nome vecchio continua a valere.
- **Comportamento:**
  - si applica appena si salva il file, come `settings.json`;
  - un errore indica la riga, e il resto del file continua a valere;
  - l'interruttore «Usa il mio stile» nelle Impostazioni lo spegne senza riavviare;
  - avviando con ⌥ premuto, Keel parte senza lo stile, per quando il file l'ha reso inutilizzabile.
- **Fasi proposte:**
  1. **Il file:** lettura con la libreria CSS di Firefox (`cssparser`), precedenza, ricarica al salvataggio, errori, interruttore, avvio con ⌥; colori e caratteri.
  2. **Le parti principali:** barra laterale, schede, barra di stato, pannello in basso, barra del titolo ed editor.
  3. **Il resto:** posizioni e pezzi da nascondere, una pagina in `docs` con tutti i nomi, suggerimenti dei nomi mentre si scrive il file.
- **Il lavoro vero:** oggi circa 176 misure sono scritte direttamente nel codice. Ogni parte va trasformata in qualcosa con un nome che il file può raggiungere. Esiste già `workbench.colorCustomizations` (solo colori, come in VS Code): lo stile personale va molto oltre.

### Fasi opzionali, solo se serviranno

- pannelli web delle estensioni con il WebKit di macOS;
- estensioni che vogliono Node completo;
- Swift e Flutter;
- suggerimenti col Tab;
- altre lingue dell'interfaccia;
- Windows e Linux;
- lavoro remoto via SSH.

### Punti aperti

- Il font del codice di serie: SF Mono oppure un altro.
- La minimappa: accesa o spenta di serie.
- La pubblicazione: **Keel è scaricabile da https://keel.mezzalab.eu dal 23 novembre (0.1.0); il 24 novembre è uscita la 0.1.1** con l'interfaccia rifatta e il benvenuto, **il 25 novembre la 0.1.2** con il pannello di Claude rifatto; si aggiorna da solo. Ogni versione va scritta in `CHANGELOG.md` e `docs/CHANGELOG.it.md` prima del rilascio. Il piano e com'è andata sono in [`PUBBLICAZIONE.md`](PUBBLICAZIONE.md), in quattro fasi da fare in ordine, ognuna col via dell'autore: aggiornamenti dentro Keel con un motore suo (crate `update`), controllati solo all'avvio (fase A fatta il 23 novembre); script di rilascio e di installazione (fase B fatta il 23 novembre: la chiave di firma è nel Portachiavi login come «Keel release signing key», e ne va tenuta una copia fuori dal Mac); pagina di download su KeelSite (fase C fatta il 23 novembre, legge `/updates.json`); sito su keel.mezzalab.eu dal mini PC, con il deploy da `main` e solo due versioni dei pacchetti. La firma di Apple non si fa, per scelta dell'autore del 25 novembre (niente account sviluppatore a 99 $ l'anno): chi installa con lo script del sito non vede l'avviso di macOS, chi scarica il DMG lo apre da Impostazioni di Sistema › Privacy e sicurezza › «Apri comunque». Dal 23 novembre l'app porta con sé gli avvisi delle licenze di terze parti (`crates/keel/THIRD-PARTY-LICENSES.txt`, scritto da `scripts/third-party-licenses.py`, visibile in Impostazioni › Aggiornamenti): vanno rigenerati quando cambiano le dipendenze, e un test e `release.sh` lo ricordano. Resta da decidere la lingua principale del sito (ora l'italiano).
- La repo `Keel` è pubblica su GitHub dal 24 novembre. **La licenza** (25 novembre, `LICENSE`, in italiano `docs/LICENZA.it.md`) è scritta su misura, perché l'autore vuole che il codice si legga ma non si riusi: tutti i diritti riservati; il codice non si può copiare fuori da GitHub, modificare, compilare, ridistribuire, vendere, mettere in altri software né usare per addestrare modelli di IA; l'app si scarica dal sito e si usa gratis, anche per lavoro, ma non si condivide altrove, non si vende e non si modifica. Restano libere le parti di altri (`vendor/gpui-base` e `THIRD-PARTY-LICENSES.txt`). Vale la legge italiana. Keel non va chiamato «open source», né sul sito né nel README. Il sito la collega nel piè di pagina.
- Il ramo `main` non esiste ancora su GitHub: il predefinito è `develop`.
- La cartella `KeelSite/` dentro Keel è un sito vecchio del 21 novembre, in Next.js, messo lì dall'autore: non è in git e non va committata. Quello buono è la repo `KeelSite` accanto a Keel.
- Il merge di `develop` su `main`: solo quando lo chiede l'autore.

---

## 5. Come si lavora

- **Una fase alla volta,** divisa in sottofasi. Non si comincia una fase senza il via dell'autore.
- **Verifica:** test automatici, anche dell'interfaccia senza aprire finestre, più build e misure. Per l'aspetto c'è `scripts/snapshots.sh`, che disegna la finestra fuori dallo schermo in immagini, senza catturare lo schermo; l'ultima parola resta all'autore.
  - Un difetto si riproduce prima con un test che fallisce, poi si corregge.
- **Commit:**
  - sul branch `develop`, con l'account dell'autore e senza attribuzioni ad assistenti;
  - in inglese, nello stile Conventional Commits;
  - i commit li fa l'assistente man mano, divisi per argomento, anche molti in un lavoro lungo;
  - il push si fa solo quando lo chiede l'autore;
  - `main` si aggiorna solo su richiesta.
- **Lingue:** codice, commenti, README e commit in inglese; i documenti in `docs/` e le risposte all'autore in italiano.
- **Prudenza:**
  - niente cancellazioni ricorsive senza chiedere;
  - si fermano solo i propri processi, per PID: sul Mac girano altri progetti;
  - le prove usano una `KEEL_DATA_DIR` a parte.
- **`PIANO.md`** si aggiorna alla fine di ogni fase o giro di ritocchi; questo documento e `DETTAGLI.md` quando cambia quello che raccontano.
- **Repo `KeelSite`:** commit su `develop`, push e poi `main`, ma solo quando lo chiede l'autore.
- **Una versione nuova:** si scrive in `CHANGELOG.md` e `docs/CHANGELOG.it.md`, si fa il commit, poi con la VPN accesa `scripts/release.sh VERSIONE note.md` (le note sono un riassunto del changelog, con `## it` e `## en`): il mini PC chiede il suo codice di verifica una volta, e lo scrive l'autore. Lo script fa da solo il commit `chore(release)` e il tag; poi il push di `develop` e del tag. Sul sito, la pagina delle novità (`keel.mezzalab.eu/changelog/`) vuole la versione nuova in `KeelSite/src/copy/it.ts` e `en.ts` (`changelogPage.releases`).
