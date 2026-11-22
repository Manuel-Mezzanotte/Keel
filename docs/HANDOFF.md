# Keel — passaggio di consegne

*Scritto il 18 novembre 2026, aggiornato il 22 novembre all'ultimo commit di `develop`.*

Questo documento serve a chi riprende il lavoro su Keel, persona o assistente, senza aver seguito la storia. Spiega:
- che cos'è il progetto e perché esiste;
- com'è fatto;
- cosa è stato fatto finora;
- cosa resta da fare.

Il dettaglio di ogni fase, con le decisioni e le prove, è in [`PIANO.md`](PIANO.md).

---

## 1. Cos'è Keel e perché esiste

Keel è un **IDE personale per macOS scritto in Rust**. L'idea è avere un editor:
- **veloce e leggero:** con lo stesso progetto e lo stesso file Keel occupa 265 MB, VS Code 1.014 e Cursor 1.320 (la colonna Memoria di Monitoraggio Attività; il confronto è al punto 3);
- **bello:** stile Apple, disposizione come Cursor, tema predefinito blu mezzanotte;
- **familiare:** stesse scorciatoie e stessi nomi dei comandi di VS Code, `settings.json` e `keybindings.json` compatibili;
- **estendibile:** le estensioni di VS Code funzionano dove possibile, e quelle dell'autore sempre. La prova del nove è **VSAuto Ghost**, la sua estensione di «fake coding».

Nasce dalla curiosità per le app native in Rust e dal desiderio di un editor tutto suo. Cursor resta installato come riserva. La pubblicazione (firma Apple, aggiornamenti automatici) non è ancora decisa: se ne parla dopo le prove a mano. La pagina di presentazione invece c'è, in una repo sua (`KeelSite`).

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
| `crates/keel` | l'app: finestre, interfaccia, tutte le viste (~81.000 righe) |
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
| `vendor/gpui-base` | copia dell'editor di gpui-component, con le correzioni che servivano (pieghe, più cursori) |
| `fixtures/` | file e pacchetti di estensioni per i test |
| `scripts/` | `bundle-macos.sh` (crea `Keel.app`), `make-dmg.sh` (crea il DMG), `measure-memory.sh` |
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
- estensioni, sessioni, progetti recenti, copie di emergenza e registri dei crash.

Per le prove si usa sempre una cartella a parte, con `KEEL_DATA_DIR`, mai quella vera.

### Comandi utili

```bash
cargo run -p keel                              # avvio in sviluppo
cargo test --workspace                         # tutti i test
cargo clippy --workspace --all-targets         # controlli
scripts/make-dmg.sh                            # Keel.app + target/release/Keel-0.1.0.dmg
scripts/snapshots.sh <cartella>                # la finestra disegnata fuori dallo schermo, in PNG
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
- **le funzioni nuove** (21–22 novembre), dal commit `50e24d1` al `2e4b0a5`, più i documenti (`1c4cd02`): 12 commit su `develop`, non ancora su GitHub. La versione con tutte queste è installata in /Applications dal 22 novembre:
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
- **la pagina di presentazione** (22 novembre), in una repo sua, `KeelSite` (privata su GitHub, sul Mac accanto a Keel): React, Vite, Tailwind e shadcn/ui, in italiano e in inglese. Dal cielo si scende nel mare, che copre una copia di Keel in HTML, navigabile e nei tre temi; poi il confronto misurato con VS Code e Cursor, le funzioni, i linguaggi, le estensioni e le domande. I numeri e come sono stati misurati stanno nel suo `src/site.ts`. Per pubblicarla servono un dominio, dove metterla e l'app firmata da Apple: finché `downloadUrl` è `null` la pagina dice «In arrivo».

Stato dei controlli all'ultimo commit:
- tutti i test passano, 542 in tutto, 300 solo nell'app;
- clippy non segnala niente;
- la prima schermata di Keel arriva in 330 ms, media di sei avvii;
- **memoria di Keel** (22 novembre, versione installata, M4 Pro, language server spenti), come la dà Monitoraggio Attività: 132 MB con una finestra vuota, 271 MB con un progetto e un file, 299 MB con 450 file e 20 schede, 326 MB con due progetti. I 130–150 MB scritti in `PIANO.md` sono la memoria residente (RSS), che non conta quella grafica: non vanno usati per i confronti;
- **confronto** (22 novembre, stesso Mac, stesso progetto e stesso file Rust, profilo vuoto, nessuna estensione, media di tre avvii):

  | | Keel | VS Code 1.125 | Cursor 3.30 |
  |---|---|---|---|
  | Memoria di tutti i processi, dopo 40 s | 265 MB | 1.014 MB | 1.320 MB |
  | Avvio | 425 ms | 653 ms | 970 ms |
  | App su disco | 137 MB | 1.096 MB | 1.446 MB |
  | Processi | 1 | 8 | 14 |

  Per Keel l'avvio è fino alla finestra sullo schermo; per VS Code e Cursor è il tempo che dichiarano loro (`--prof-append-timers`), e la loro finestra vuota compare più o meno quando quella di Keel. Su disco VS Code e Cursor sono universali, Keel è solo per Apple Silicon.

---

## 4. Cosa resta da fare

### Subito: le prove a mano

Le fasi sono fatte, ma l'autore deve ancora provarle a mano. L'elenco di ogni fase è in `PIANO.md`, nei punti «Le tue prove a mano». I difetti trovati diventano ritocchi, come quelli del 18 novembre.

Delle funzioni nuove restano da provare davvero due cose, che qui non si potevano provare:
- **il debug di Node e TypeScript:** vuole `js-debug` di Microsoft, che su questo Mac non c'è. Keel lo scarica da GitHub (1,2 MB) solo quando glielo si chiede, e controlla che il file abbia l'impronta pubblicata;
- **le azioni che scrivono su GitHub:** commenti, approvazioni, unione e creazione di una pull request sono provate solo con un `gh` finto.

### Difetti trovati, da correggere

Trovati il 22 novembre misurando e fotografando Keel per la pagina di presentazione:
- **memoria grafica:** aprendo un file la parte grafica cresce di circa 125 MB. La causa non è ancora accertata; il sospetto è il disegno dei tracciati (`paint_path`), che fa riservare a GPUI delle immagini grandi quanto la finestra;
- **CPU da fermo:** con un file aperto e la finestra in primo piano Keel usa circa il 5% di un core anche senza fare niente (senza file lo 0%); dal campionamento sembra che ridisegni di continuo;
- **test di un crate in una sottocartella senza workspace:** non partono, perché `cargo test -p` viene lanciato dalla cartella del progetto.

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
- La pubblicazione: account sviluppatore Apple, firma, notarizzazione, aggiornamenti automatici; per la pagina anche il dominio, dove metterla, il prezzo o la licenza e quale lingua tenere come principale (ora l'italiano).
- I 12 commit di `develop` da mandare su GitHub, dopo le prove dell'autore.
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
  - il push si fa solo quando lo chiede l'autore;
  - `main` si aggiorna solo su richiesta.
- **Lingue:** codice, commenti, README e commit in inglese; i documenti in `docs/` e le risposte all'autore in italiano.
- **Prudenza:**
  - niente cancellazioni ricorsive senza chiedere;
  - si fermano solo i propri processi, per PID: sul Mac girano altri progetti;
  - le prove usano una `KEEL_DATA_DIR` a parte.
- **`PIANO.md`** si aggiorna alla fine di ogni fase o giro di ritocchi; questo documento e `DETTAGLI.md` quando cambia quello che raccontano.
- **Repo `KeelSite`:** commit su `develop`, push e poi `main`, ma solo quando lo chiede l'autore.
