# Keel — piano di sviluppo

Keel è il tuo IDE personale: scritto in Rust, veloce e leggero, bello come Cursor e in stile Apple, con le estensioni di VS Code dove possibile.

Si lavora **una fase alla volta**. All'inizio di ogni fase ne dettagliamo i passi; alla fine la provi tu, e si passa alla successiva solo quando ti convince. La pubblicazione dell'app non è in questo piano: ci si pensa alla fine, dopo i tuoi test.

*Scritto il 14 novembre 2026, aggiornato il 15 novembre. Stato: fase 0 in corso.*

---

## Principi

- **Veloce e leggero.** L'editor non ti fa mai aspettare e usa una frazione della RAM di Cursor.
- **Bello e pulito.** Stile Apple e Cursor: essenziale, curato nei dettagli, tema predefinito blu mezzanotte.
- **Familiare.** Disposizione e scorciatoie di VS Code e Cursor: ti ci trovi dal primo giorno.
- **Affidabile.** Non perde mai un file né una modifica.
- **Estendibile.** Le estensioni di VS Code funzionano dove possibile; le tue, come VSAuto Ghost, sempre.
- **Mac prima di tutto.** Solo macOS è obbligatorio; il codice resta portabile per Windows e Linux, un giorno.

## Decisioni prese

- **Base.** VS Code non si può portare in Rust. Un fork di VS Code sarebbe Electron come Cursor, e un fork completo di Zed è enorme e sotto GPL. Keel si costruisce su **GPUI**, il motore grafico di Zed, e su **gpui-component**: entrambi con licenza Apache.
- **Cosa aspettarsi.** L'editor occupa molta meno RAM di Cursor. I language server e `pnpm run dev` restano gli stessi con qualsiasi editor.
- **Estensioni di VS Code, tre «prese»:**
  1. lettore di VSIX per temi, icone, snippet e grammatiche;
  2. gli stessi language server e debugger che usa VS Code;
  3. un ponte `vscode` in JavaScript, costruito a pezzi, per le estensioni con codice.

  I VSIX arrivano da Open VSX: il marketplace di Microsoft è vietato agli altri editor.
- **Fuori portata:** Jupyter, Remote SSH e Dev Containers di Cursor, Gemini Code Assist. Le estensioni con pannelli web (Git Graph, Markdown Preview Enhanced…) diventano funzioni native di Keel.
- **VSAuto Ghost è la prova del nove.** Usa circa 55 funzioni dell'API di VS Code e richiede solo `vscode`, `path`, `crypto` e il suo `typescript.js` da 8,6 MB. Gira nel motore V8 dentro Keel, senza Node completo.
- **IA:** Claude Code nel terminale e un pannello agente. Niente suggerimenti col Tab, per ora.
- **Lingue dell'interfaccia:** italiano e inglese obbligatori, le altre facoltative.
- **Aspetto:** disposizione come Cursor, stile Apple; temi Mezzanotte (predefinito), Chiaro e Nero, più quelli importati.
- **Sistemi:** solo Mac obbligatorio.
- **Verifica:** test automatici e misure, niente screenshot. L'aspetto lo controlli tu.
- **Commit:** li faccio io sul branch `develop`, con il tuo account e senza attribuzioni a Claude, in inglese e nello stile Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`, `test:`…). Uno per ogni sottofase o pezzo importante, con push su GitHub dopo ogni commit. `main` si aggiorna solo quando lo dici tu.
- **Lingua del codice:** nomi, commenti, README e commit in inglese; i documenti in `docs/` in italiano.
- **Identità:** nome Keel, app `Keel.app`, identificativo `com.manuelmezzanotte.keel` (come `com.manuelmezzanotte.sail` del browser), comando `keel` nel terminale.

## Lo stack

Versioni lette su crates.io il 14 e il 15 novembre 2026; si bloccano quando entrano nel progetto.

| Cosa | Libreria | Versione | Licenza |
|---|---|---|---|
| Linguaggio | Rust, fissato in `rust-toolchain.toml` | 1.100.0 | — |
| Interfaccia (disegna con la scheda video, Metal) | `gpui-pre`, istantanea del GPUI di Zed usata da gpui-component | 0.3.12 | Apache-2.0 |
| Piattaforma macOS di GPUI (finestre, testo) | `gpui-pre-platform` | 0.3.12 | Apache-2.0 |
| Componenti ed editor di codice | `gpui-component` (l'editor sta in `gpui-base`) | 0.9.1 | Apache-2.0 |
| Icone dei componenti | `gpui-kit-assets` | 0.9.1 | Apache-2.0 |
| Colori della sintassi | `tree-sitter` più le grammatiche, la versione di gpui-component | 0.26 | MIT |
| Testo in memoria (dentro gpui-component) | `ropey` | 2.0 beta | MIT/Apache |
| Linguaggi (client LSP nostro) | `lsp-types` | 0.97.0 | MIT |
| Terminale | `alacritty_terminal` | 0.26.0 | Apache-2.0 |
| Git (lettura) più il comando `git` (scrittura) | `gix` | 0.90.0 | MIT/Apache |
| Differenze tra file | `imara-diff` | da fissare | Apache-2.0 |
| Ricerca | `ignore`, `grep-searcher`, `nucleo` | 0.4.34 · 0.1.17 · 0.5.0 | MIT · MIT · MPL-2.0 |
| Cambi su disco | `notify` | 8.2.2 | CC0 |
| Estensioni (motore JavaScript V8) | `deno_core` | 0.413.0 | MIT |
| Pannello agente | `agent-client-protocol` | 5.3.0 | Apache-2.0 |
| Anteprima Markdown | `pulldown-cmark` o la vista di gpui-component | da fissare | MIT |
| PDF | PDFKit di macOS (`objc2-pdf-kit`) | da fissare | MIT |
| Pacchetto `Keel.app` | `cargo-packager` | da fissare | — |
| Pannelli web (solo fase opzionale) | `wry` | 0.57.2 | MIT/Apache |

gpui-component ha già pronte le grammatiche tree-sitter di quasi tutti i linguaggi che ti servono (TypeScript, TSX, JavaScript, HTML, CSS, Rust, Python, Java, C, C++, C#, Bash, SQL, YAML, TOML, Markdown…). Ha anche il supporto alle grammatiche TextMate (`syntect`), utile come riserva per le grammatiche dei VSIX.

## Architettura

Un workspace Cargo diviso in pezzi (crate), ognuno con un compito:

| Crate | Cosa contiene | Nasce nella fase |
|---|---|---|
| `keel` | avvio, finestre, menu di macOS, comando `keel` | 0 |
| `editor` | editor di codice, decorazioni, testo fantasma | 0 |
| `extensions` | lettore VSIX, motore V8, ponte `vscode`, Open VSX | 0 |
| `extensions/src/js` | il modulo `vscode` finto e il minimo di Node, scritti in JavaScript | 0 |
| `language` | grammatiche tree-sitter e registro dei linguaggi | 0 |
| `ui` | sistema di design, temi, icone, componenti comuni | 1 |
| `settings` | impostazioni, scorciatoie con «when», lingue | 1 |
| `workspace` | disposizione: barre, pannelli, gruppi di editor, sessione | 1 |
| `project` | cartelle, albero dei file, osservatore del disco | 2 |
| `search` | apertura veloce e ricerca nei file | 3 |
| `terminal` | terminali e porte | 4 |
| `git` | stato, differenze, commit, grafico | 5 |
| `lsp` | client LSP e installazione dei server | 6 |
| `preview` | anteprima Markdown, PDF e immagini | 7 |
| `ai` | collegamento a Claude Code e pannello agente | 10 |

```
Keel.app (Rust)
├── Interfaccia: GPUI + gpui-component (editor, pannelli, temi)
├── progetto · ricerca · terminale · git
├── linguaggi ──► language server (programmi esterni: vtsls, rust-analyzer…)
├── estensioni ─► motore V8 in un thread separato + ponte `vscode`
└── IA ─────────► Claude Code (terminale collegato + pannello ACP)
```

## Obiettivi di prestazione

Da confermare con le misure della fase 0. Si misura con lo stesso script su Keel e su Cursor; language server e terminali si contano a parte, perché sono uguali nei due editor.

| Misura | Obiettivo |
|---|---|
| Avvio con un progetto aperto | meno di 1 secondo |
| RAM di Keel con un progetto web aperto | sotto i 500 MB |
| Scrittura e scorrimento | nessun ritardo percepibile: ogni tasto a schermo entro un fotogramma |
| Ricerca nei file di un tuo progetto | risultati in meno di 1 secondo |
| File da 100 MB | si apre e scorre senza bloccarsi |

---

## Le fasi

| Fase | Nome | Dimensione | Stato |
|---|---|---|---|
| 0 | Prototipo e prova a tempo | 2–3 settimane | in corso |
| 1 | Fondamenta e design | media | da fare |
| 2 | Editor e file | grande | da fare |
| 3 | Ricerca | piccola | da fare |
| 4 | Terminale e pannello in basso | media | da fare |
| 5 | Git: modifiche e commit | media | da fare |
| 6 | Linguaggi principali: web e Rust | grande | da fare |
| 7 | Anteprima Markdown e PDF | media | da fare |
| 8 | Grafico e storico git | media | da fare |
| 9 | Estensioni | grande | da fare |
| 10 | IA: Claude Code | media | da fare |
| 11 | Altri linguaggi | media | da fare |
| 12 | Rifinitura, prestazioni e affidabilità | grande | da fare |

Due traguardi: dopo la **fase 0** si decide se continuare; dopo la **fase 6** Keel è usabile tutti i giorni per i progetti web, con Cursor di riserva.

### Fase 0 — Prototipo e prova a tempo

**Obiettivo.** Verificare subito le due cose più rischiose: l'editor di gpui-component e il ponte per le estensioni di VS Code. Alla fine si decide se continuare.

**Cosa si fa**
- **Repo pronta:** workspace Cargo, versione di Rust fissata (`rust-toolchain.toml`), `.gitignore`, README.
- **Finestra minima,** con le versioni di GPUI e gpui-component bloccate: Keel apre un file (anche da terminale, con `keel file.tsx`), lo modifica e lo salva, con i colori della sintassi per TypeScript e TSX.
- **Motore delle estensioni:** V8 (`deno_core`) in un thread separato, il modulo `vscode` finto con le funzioni che usa VSAuto Ghost, la lettura del suo VSIX.
- **VSAuto Ghost 0.6.1** caricata dal VSIX e funzionante: scrittura a ogni tasto, suggerimenti grigi con Tab e Cmd+→, menu dei suggerimenti, barra verde, pausa e stop.
- **Script di misura** per RAM, avvio e fluidità, da usare uguale su Keel e su Cursor.
- **Prove mirate:** tastiera italiana (è, à, accenti con i tasti morti), più cursori, file da decine di migliaia di righe, scorrimento veloce.

**Sottofasi** (ognuna chiusa da uno o più commit)

| Sottofase | Cosa | Stato |
|---|---|---|
| 0.1 | Fondamenta della repo: branch `develop`, workspace Cargo, Rust fissato, README, VSIX di VSAuto Ghost tra i file di prova | fatta |
| 0.2 | Finestra ed editor: GPUI e gpui-component, apertura di un file da riga di comando, modifica e salvataggio, colori TypeScript e TSX | fatta |
| 0.3 | Prove dell'editor: tastiera italiana e accenti, più cursori, file grandi, scorrimento | fatta |
| 0.4 | Motore delle estensioni: V8 in un thread separato, lettore di VSIX, primo modulo `vscode` | fatta |
| 0.5 | Ponte per VSAuto Ghost: le circa 55 funzioni dell'API di VS Code che usa | da fare |
| 0.6 | VSAuto Ghost funzionante da capo a fondo | da fare |
| 0.7 | Misure contro Cursor, resoconto e decisione se continuare | da fare |

**Note dalla 0.2**
- **Cosa ha già l'editor di gpui-component:** più cursori (Alt+clic, Cmd+Alt+↑/↓), accenti con i tasti morti, pieghe dei blocchi, cerca e sostituisci (senza regex), annulla fino a 1000 passi, menu dei suggerimenti, errori sottolineati, decorazioni.
- **Cosa manca:** i suggerimenti in linea (inlay hints) e un modo diretto per mostrare il testo grigio. Oggi il testo grigio passa solo dal fornitore di completamenti, con 300 ms di attesa. Per VSAuto Ghost va valutato nella 0.5.
- **Limite dichiarato:** circa 50.000 righe per file; si misura nella 0.3.
- **Colori del TSX:** gpui-component dava al TSX solo poche regole. Keel aggiunge quelle complete di TypeScript più tag e attributi JSX, verificate dai test.
- **Prima misura:** Keel in versione di sviluppo, con un file TSX aperto, occupa circa 116 MB di RAM.

**Note dalla 0.3**
- **Verificato con i test:** lettere accentate italiane, accenti composti con i tasti morti, più cursori (Cmd+Alt+↓), fine riga di Windows mantenuti, annulla e ripeti.
- **File grandi** (versione ottimizzata, test senza finestra):

  | File TypeScript | Apertura | Un tasto |
  |---|---|---|
  | 50.000 righe | 148 ms | 1,6 ms |
  | 200.000 righe | 426 ms | 1,1 ms |

  Il limite dichiarato di circa 50.000 righe non è un muro. I tempi non comprendono il disegno sullo schermo: la fluidità dello scorrimento la provi tu.
- **Comandi che l'editor non ha ancora** e che aggiungiamo nella fase 2: commenta (Cmd+/), sposta e duplica righe, seleziona la prossima occorrenza (Cmd+D), vai alla riga.

**Note dalla 0.4**
- **Il motore:** V8 gira in un thread suo, con deno_core. Keel e le estensioni si parlano con messaggi JSON: avvisi senza risposta, oppure chiamate con risposta.
- **Il ponte `vscode` è in JavaScript, non in TypeScript:** così la compilazione di Keel non ha bisogno di Node. Sta in `crates/extensions/src/js`, insieme al minimo di Node che serve (`require`, `Buffer`, timer, `console`, `path`, `crypto` per SHA-256).
- **Sicurezza:** un'estensione può caricare moduli solo dalla propria cartella. Il lettore di VSIX rifiuta i file che uscirebbero dalla cartella di destinazione e i pacchetti oltre 1 GB.
- **VSAuto Ghost si attiva già:** registra i suoi 16 comandi, compreso `type`, che in VS Code intercetta ogni tasto, più il tipo di decorazione, il file system `vsauto-ghost:` e il fornitore di suggerimenti.
- **Cosa resta per la 0.5:** collegare tutto all'editor vero. Documenti ed editor visti dall'estensione, modifiche, testo grigio, menu dei suggerimenti, barra verde, barra di stato, scelta rapida, schede, e i tasti che passano da `type`.

**Fatto quando** (criterio per continuare)
- Scrivere e scorrere è fluido, anche con gli accenti.
- VSAuto Ghost funziona come in Cursor.
- La RAM di Keel, misurata con lo stesso script, è ben sotto quella di Cursor.

**Se non convince.** Si valuta cosa cambiare, per esempio un editor nostro sopra GPUI o QuickJS al posto di V8, oppure si ripiega su Zed più un programma esterno per VSAuto Ghost.

### Fase 1 — Fondamenta e design

**Obiettivo.** Lo scheletro definitivo dell'app e il suo aspetto, prima di riempirlo di funzioni.

**Cosa si fa**
- **Bozze:** 2–3 proposte di interfaccia in stile Apple e Cursor. Scegli tu, prima di costruire.
- **Sistema di design:** colori, testi (SF Pro per l'interfaccia, SF Mono per il codice, altri font a scelta), spaziature, angoli, icone (Lucide), animazioni discrete.
- **Temi:**
  - **Mezzanotte**, predefinito, con i blu di Sail (`#070B14`, `#0B1426`, `#1E2B47`);
  - **Chiaro**;
  - **Nero**, con i neri di Sail (`#050505`, `#0A0A0A`, `#2A2A2C`).

  Keel può seguire chiaro e scuro di macOS. Il formato è compatibile con i temi di VS Code, così più avanti si importano quelli dei VSIX.
- **Disposizione come Cursor:** barra delle attività, barra laterale, area degli editor, pannello in basso, barra di stato, barra del titolo integrata con i semafori di macOS, palette dei comandi.
- **Logo e icona di Keel**, con un marchio proprio.
- **Infrastruttura:**
  - registro dei comandi;
  - scorciatoie con le condizioni «when» nel formato di VS Code, partendo da quelle di VS Code per Mac e tenendo conto della tastiera italiana;
  - impostazioni in JSON per utente e per progetto (legge anche `.vscode/settings.json`);
  - italiano e inglese, con la scelta nelle impostazioni e come predefinita la lingua del Mac;
  - registri e gestione degli errori.
- **Menu di macOS:** Keel, File, Modifica, Selezione, Vista, Vai, Terminale, Finestra, Aiuto.

**Fatto quando**
- La finestra ha la disposizione definitiva e ti piace.
- I tre temi e le due lingue si cambiano al volo.
- Le scorciatoie di base funzionano con la tastiera italiana.

### Fase 2 — Editor e file

**Obiettivo.** Lavorare sui file di un progetto come in Cursor.

**Cosa si fa**
- **Progetti:** apri cartella, progetti recenti, schermata iniziale, una finestra per progetto, comando `keel .` dal terminale (come `code .`).
- **Esplora file:** albero con le icone; crea, rinomina, sposta, elimina (nel Cestino); trascina e rilascia; mostra nel Finder; copia il percorso; file ignorati da git attenuati.
- **Schede:** anteprima, fissate, pallino delle modifiche, chiudi le altre, riapri l'ultima chiusa; editor affiancati e gruppi.
- **Scrittura:**
  - più cursori e selezione a colonna;
  - cerca e sostituisci nel file, anche con regex; vai alla riga;
  - commenta; sposta e duplica le righe; piega i blocchi;
  - parentesi abbinate; chiusura automatica di parentesi e tag JSX/HTML;
  - rientri automatici e guide; a capo automatico;
  - sticky scroll, breadcrumb, zoom, minimappa attivabile.
- **Navigazione:** apertura veloce (Cmd+P) e palette dei comandi (Cmd+Shift+P).
- **Cambi dall'esterno:** quando un file cambia fuori da Keel, per esempio perché lo modifica Claude Code, si ricarica da solo. Se avevi modifiche non salvate, Keel te lo segnala.
- **Mai perdere niente:** salvataggio automatico attivabile, modifiche non salvate conservate dopo una chiusura o un crash, ripristino di schede e disposizione.
- **Altri file:** file molto grandi, anteprima delle immagini, avviso per i file binari.

**Fatto quando**
- Apri GestionaleHospitality e ci lavori (navighi, modifichi, salvi) come in Cursor, a parte gli aiuti dei linguaggi che arrivano nella fase 6.
- Claude Code modifica un file aperto e Keel lo aggiorna subito.

### Fase 3 — Ricerca

**Obiettivo.** Trovare qualsiasi cosa nel progetto in un attimo.

**Cosa si fa**
- **Cerca nei file** (Cmd+Shift+F): maiuscole e minuscole, parola intera, regex; cartelle da includere ed escludere; rispetta `.gitignore`; risultati raggruppati per file, con anteprima.
- **Sostituisci in tutti i file**, con l'anteprima delle modifiche.
- **Apertura veloce migliorata:** file recenti in cima, ricerca anche nel percorso.
- La ricerca dei simboli (funzioni, classi) arriva con i linguaggi, nella fase 6.

**Fatto quando**
- Una parola in GestionaleHospitality si trova in meno di un secondo.
- Una sostituzione su più file si controlla prima di applicarla.

### Fase 4 — Terminale e pannello in basso

**Obiettivo.** Lanciare `pnpm run dev` e Claude Code senza uscire da Keel.

**Cosa si fa**
- **Terminale integrato** (`alacritty_terminal`): più terminali, schede, split, rinomina e chiudi; zsh con la cartella corrente; colori del tema; ricerca; copia e incolla; link cliccabili (`file:riga` apre l'editor, gli indirizzi web il browser).
- **Claude Code perfetto nel terminale:** colori, mouse, incolla di testi lunghi e immagini, ridimensionamento, scorciatoie. Comando «Apri Claude Code» in un terminale dedicato, a lato o in basso.
- **Pannello in basso come VS Code:**
  - **Terminale**;
  - **Output**, con i registri di Keel, dei language server e delle estensioni;
  - **Problemi**, che si riempie nella fase 6;
  - **Porte**: le porte aperte dai processi dei terminali, per esempio la 3000 di `pnpm run dev`, da aprire nel browser con un clic.
- **Script di `package.json`** lanciabili da un elenco, come «NPM Scripts» di VS Code.

**Fatto quando**
- `pnpm run dev` e Claude Code girano dentro Keel esattamente come nel Terminale di macOS.
- La porta 3000 compare in Porte e si apre nel browser con un clic.

### Fase 5 — Git: modifiche e commit

**Obiettivo.** Controllare le modifiche e fare i commit senza uscire da Keel.

**Cosa si fa**
- **Controllo sorgente:** file modificati, in stage e non tracciati; clic su un file per vedere le differenze, affiancate o in linea.
- **Stage e unstage** di file interi o di singoli blocchi; scarta le modifiche, con conferma.
- **Commit** con messaggio, anche «amend»; push, pull, fetch, sincronizza; branch dalla barra di stato (cambia, crea); stash.
- **Margine dell'editor:** righe aggiunte in verde, modificate e cancellate, con anteprima del blocco cambiato.
- **Conflitti di merge:** accetta la versione corrente, quella in arrivo o entrambe.
- **Numero di modifiche** sull'icona di git nella barra delle attività.

**Fatto quando**
- Fai un giro completo solo da Keel: modifichi, controlli le differenze, metti in stage, committi, fai push.

### Fase 6 — Linguaggi principali: web e Rust

**Obiettivo.** Gli aiuti dei linguaggi come in Cursor per il tuo lavoro di ogni giorno. **Da qui Keel è usabile tutti i giorni per i progetti web**, con Cursor di riserva.

**Cosa si fa**
- **Client LSP completo** (LSP è il protocollo con cui l'editor parla con i language server):
  - errori e avvisi sottolineati e nel pannello Problemi;
  - completamenti con snippet; info al passaggio del mouse; firma delle funzioni;
  - vai a definizione, riferimenti e implementazione; rinomina;
  - azioni rapide (correzioni, import automatici);
  - formattazione, anche al salvataggio;
  - struttura del file, suggerimenti in linea, ricerca dei simboli (Cmd+Shift+O, Cmd+T).
- **Gestione dei server:** Keel li trova o li installa da solo (npm, rustup), li riavvia se si bloccano e ne scrive i registri nell'Output.
- **Web:** TypeScript, JavaScript e React (`vtsls`); HTML, CSS e JSON (`vscode-langservers-extracted`); Emmet; Tailwind; ESLint; Prettier (quello del progetto); Prisma; Markdown (`marksman`); YAML; TOML; `.env`.
- **Rust:** `rust-analyzer`, così Keel si può sviluppare dentro Keel.
- **Snippet** nel formato di VS Code.

**Fatto quando**
- Su GestionaleHospitality e GestionaleSalon Keel mostra gli stessi errori di Cursor, completa, formatta al salvataggio, rinomina e trova i riferimenti.
- Su Keel stesso `rust-analyzer` funziona.

### Fase 7 — Anteprima Markdown e PDF

**Obiettivo.** Leggere i Markdown e i PDF dentro Keel.

**Cosa si fa**
- **Anteprima Markdown** affiancata e aggiornata mentre scrivi:
  - titoli, tabelle, liste di controllo, blocchi di codice colorati, immagini, link, citazioni (stile GitHub);
  - scorrimento sincronizzato tra editor e anteprima;
  - anteprima a tutta scheda;
  - colori coerenti con il tema.
- **PDF in una scheda**, con PDFKit di macOS: pagine, zoom, miniature, ricerca nel testo, copia del testo.
- **Immagini e SVG** con zoom.

**Fatto quando**
- I tuoi file `.md` si leggono almeno bene come in Cursor e i PDF si aprono dentro Keel.

### Fase 8 — Grafico e storico git

**Obiettivo.** Tutto quello che oggi fai con Git Graph, nativo in Keel.

**Cosa si fa**
- **Grafico** di branch e commit con linee colorate, autore, data e messaggio; filtri per branch; ricerca nei commit.
- **Dettaglio di un commit:** file cambiati e differenze.
- **Azioni dal grafico:** checkout, crea branch o tag, copia l'hash, cherry-pick, revert (con conferma).
- **Storico di un file** e autore di ogni riga (blame, attivabile).

**Fatto quando**
- Fai tutto quello che fai oggi con Git Graph.

### Fase 9 — Estensioni

**Obiettivo.** Gestire le estensioni come in Cursor e far funzionare quelle che ti servono.

**Cosa si fa**
- **Vista Estensioni:** installate, attiva e disattiva (anche per progetto), disinstalla, dettagli con il README, impostazioni, aggiornamenti.
- **Installazione** da file VSIX e da Open VSX, con la ricerca dentro Keel.
- **Le tre prese complete:**
  - temi, icone dei file (Material Icon Theme), snippet, grammatiche e configurazioni di linguaggio dai VSIX;
  - estensioni di linguaggio collegate ai loro server;
  - ponte `vscode` allargato con le funzioni che servono.
- **Isolamento:** un'estensione lenta o in crash non blocca Keel e si riavvia da sola.
- **Importa da Cursor** l'elenco delle estensioni: installa quelle disponibili su Open VSX e segnala quelle che non possono funzionare.
- **Prove su estensioni vere:** VSAuto Ghost, Dracula, Material Icon Theme, Peacock, gitignore, markdownlint.

**Fatto quando**
- VSAuto Ghost si installa dalla vista Estensioni come in Cursor e le estensioni della lista funzionano.

### Fase 10 — IA: Claude Code

**Obiettivo.** Lavorare con Claude Code dentro Keel, vedendo e controllando ogni modifica.

**Cosa si fa**
- **Claude Code nel terminale, collegato a Keel.** È lo stesso collegamento che Claude Code usa con VS Code e JetBrains: un WebSocket annunciato da un file in `~/.claude/ide/`. Claude vede il file aperto e la selezione e legge i Problemi; le modifiche che propone si aprono in Keel come differenze da accettare o rifiutare.
- **Pannello agente con ACP** (Agent Client Protocol, il protocollo aperto di Zed) e l'adattatore `@agentclientprotocol/claude-agent-acp`:
  - conversazione con Claude Code in un pannello laterale;
  - passaggi e piano visibili;
  - modifiche file per file da accettare o rifiutare;
  - richieste di permesso;
  - riferimenti a file e selezioni;
  - cronologia delle conversazioni.
- **Il tuo account di Claude Code:** nessuna chiave dentro Keel.

**Fatto quando**
- Lavori con Claude Code dal terminale e dal pannello, e ogni modifica la vedi e la controlli in Keel.

### Fase 11 — Altri linguaggi

**Obiettivo.** Tutti gli altri linguaggi con cui lavori.

**Cosa si fa**
- **Python** (`basedpyright` e `ruff`), **Java** (`jdtls`) e **Spring Boot**, **C e C++** (`clangd`), **C#** (server Roslyn o `csharp-ls`; serve .NET).
- **Linguaggi di base:** Bash e zsh, Dockerfile, SQL, XML, Makefile.
- **Per ognuno:** colori, errori, completamenti, formattazione. Se il server manca, Keel ti guida a installarlo.

**Fatto quando**
- Per ogni linguaggio un progetto di prova si comporta come in VS Code.

### Fase 12 — Rifinitura, prestazioni e affidabilità

**Obiettivo.** Da «funziona» a «perfetto».

**Cosa si fa**
- **Prestazioni:** misure finali contro Cursor (avvio, RAM, scrittura, ricerca); progetti e file enormi; sessioni lunghe senza perdite di memoria.
- **Affidabilità:** salvataggi sicuri, copie delle modifiche non salvate, ripresa dopo un crash, registro locale dei crash.
- **Impostazioni con interfaccia grafica** oltre al JSON, ed editor delle scorciatoie.
- **Importa da Cursor** impostazioni, scorciatoie e progetti recenti.
- **Rifinitura del design:** animazioni, schermate vuote, coerenza di temi e icone.
- **Accessibilità** di base, per quanto GPUI lo permette.
- **Revisione** di tutti i testi in italiano e in inglese.

**Fatto quando**
- Usi Keel per qualche settimana al posto di Cursor senza dover tornare indietro.

---

## Fasi opzionali

Da fare dopo, solo se servono:

- **Debugger** con breakpoint e **Debug Console**, con il protocollo DAP di VS Code (js-debug, CodeLLDB, debugpy).
- **Pannelli web delle estensioni**, con il WebKit di macOS (`wry`): Postman, diagrammi Mermaid nell'anteprima Markdown.
- **Estensioni che richiedono Node completo**, con un processo Node separato come fa VS Code.
- **Swift** (`sourcekit-lsp`) e **Flutter/Dart**.
- **Suggerimenti col Tab.**
- **Altre lingue** dell'interfaccia.
- **Windows e Linux.**
- **Lavoro remoto via SSH**: per ora non ti serve.

## Non in questo piano: la pubblicazione

Firma e notarizzazione di Apple, aggiornamenti automatici, un eventuale sito: ci pensiamo alla fine, dopo i tuoi test.

## Come lavoriamo

- **Una fase alla volta.** All'inizio di una fase ne dettagliamo i passi. Non si comincia la successiva finché la precedente non ti convince.
- **Controlli:** test automatici (anche dell'interfaccia, senza aprire finestre), build, e misure di RAM e velocità con lo stesso script. Niente screenshot: l'aspetto lo controlli tu.
- **Fine fase:** ti lascio le prove da fare a mano (i punti «Fatto quando») e un riassunto di cosa è cambiato.
- **VSAuto Ghost come prova fissa:** dalla fase 0 in poi deve funzionare sempre.
- **Sottofasi e commit:** ogni fase si divide in sottofasi. Alla fine di ogni sottofase, o di un pezzo importante, faccio il commit su `develop` e il push su GitHub. Il merge su `main` lo faccio solo quando me lo chiedi.
- **Questo documento** si aggiorna alla fine di ogni fase: stato, decisioni, cose rimandate.

## Rischi e come li gestiamo

| Rischio | Cosa facciamo |
|---|---|
| GPUI e gpui-component sono giovani e cambiano spesso | Versioni bloccate; aggiornamenti solo voluti, con i test |
| All'editor di gpui-component manca qualcosa (più cursori, accenti, pieghe…) | Si verifica nella fase 0; se serve lo estendiamo o scriviamo il nostro sopra GPUI |
| Scorciatoie con la tastiera italiana (per esempio Cmd+/, che sulla tastiera italiana è Shift+7) | Scorciatoie che tengono conto della tastiera, provate dalla fase 0 |
| V8 pesa: compilazione lunga e app più grande di circa 30 MB | Accettabile; QuickJS come riserva |
| L'API di VS Code è enorme | Solo le parti che servono, a pezzi |
| Il collegamento IDE di Claude Code non è documentato ufficialmente | Isolato in un modulo, seguendo le implementazioni esistenti; il pannello ACP è la strada stabile |
| Licenze | Niente codice copiato dall'editor di Zed (GPL); GPUI, gpui-component, ACP e alacritty_terminal sono Apache o MIT |
| Marketplace di Microsoft vietato | Open VSX |
| Perdita di dati | Salvataggi sicuri, copie di emergenza, test dedicati al salvataggio |
| Il progetto si allarga troppo | Fasi chiuse una alla volta; le cose in più vanno nelle opzionali |

## Punti aperti

- **Debug Console:** l'hai citata nel pannello in basso, ma senza debugger resta vuota. Proposta: arriva con il debugger opzionale.
- **Bozze della fase 1:** pagine HTML statiche, più veloci da cambiare, oppure direttamente in Keel.
- **Libreria per le lingue:** Fluent di Mozilla o `rust-i18n`, da scegliere nella fase 1.
- **Font del codice:** SF Mono di serie, oppure un altro (JetBrains Mono, Geist Mono…).
- **Minimappa:** accesa o spenta di serie.

## Riferimenti

- [gpui su crates.io](https://crates.io/crates/gpui)
- [gpui-component](https://github.com/longbridge/gpui-component)
- [agent-client-protocol su crates.io](https://crates.io/crates/agent-client-protocol)
- [Adattatore ACP per Claude Code](https://www.npmjs.com/package/@agentclientprotocol/claude-agent-acp)
- [Protocollo IDE di Claude Code, descritto da claudecode.nvim](https://github.com/coder/claudecode.nvim/blob/main/PROTOCOL.md)
- [Open VSX](https://open-vsx.org)
- [Zed](https://github.com/zed-industries/zed): da guardare per ispirazione, senza copiarne il codice (GPL)
