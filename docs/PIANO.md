# Keel — piano di sviluppo

Keel è il tuo IDE personale: scritto in Rust, veloce e leggero, bello come Cursor e in stile Apple, con le estensioni di VS Code dove possibile.

Si lavora **una fase alla volta**. All'inizio di ogni fase ne dettagliamo i passi; alla fine la provi tu, e si passa alla successiva solo quando ti convince. La pubblicazione dell'app non è in questo piano: ci si pensa alla fine, dopo i tuoi test.

*Scritto il 14 novembre 2026, aggiornato il 16 novembre. Stato: fasi 0–8 fatte, restano le tue prove a mano.*

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
| 0 | Prototipo e prova a tempo | 2–3 settimane | fatta; restano le tue prove a mano |
| 1 | Fondamenta e design | media | fatta; restano le tue prove a mano |
| 2 | Editor e file | grande | fatta; restano le tue prove a mano |
| 3 | Ricerca | piccola | fatta; restano le tue prove a mano |
| 4 | Terminale e pannello in basso | media | fatta; restano le tue prove a mano |
| 5 | Git: modifiche e commit | media | fatta; restano le tue prove a mano |
| 6 | Linguaggi principali: web e Rust | grande | fatta; restano le tue prove a mano |
| 7 | Anteprima Markdown e PDF | media | fatta; restano le tue prove a mano |
| 8 | Grafico e storico git | media | fatta; restano le tue prove a mano |
| 9 | Estensioni | grande | fatta; restano le tue prove a mano |
| 10 | IA: Claude Code | media | fatta; restano le tue prove a mano |
| 11 | Altri linguaggi | media | fatta; restano le tue prove a mano |
| 12 | Rifinitura, prestazioni e affidabilità | grande | fatta; restano le tue prove a mano |

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
| 0.5 | Ponte per VSAuto Ghost: le circa 55 funzioni dell'API di VS Code che usa | fatta |
| 0.6 | VSAuto Ghost funzionante da capo a fondo | fatta (test automatico); manca la tua prova a mano |
| 0.7 | Misure contro Cursor, resoconto e decisione se continuare | fatta |

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

**Note dalla 0.5 e 0.6**
- **La finestra è diventata un piccolo workbench:**
  - editor affiancati, con il nome del file in alto e un pallino se ci sono modifiche;
  - barra di stato in basso;
  - palette dei comandi (Cmd+Shift+P) e scelta rapida delle estensioni, fatte con la palette di gpui-component.
- **Estensioni installate come in VS Code:** `keel --install-extension file.vsix` le mette in `~/Library/Application Support/com.manuelmezzanotte.keel/extensions`. Una versione nuova va accanto alla vecchia, senza cancellarla, e all'avvio vince la più recente.
- **Cosa fa il ponte:**
  - documenti ed editor;
  - modifiche (`applyEdit`) e selezioni;
  - testo grigio con le decorazioni;
  - barra verde delle righe aggiunte, dal confronto con la versione originale dell'estensione;
  - menu dei suggerimenti alimentato dall'estensione;
  - chiavi di contesto (`setContext`) e scorciatoie con condizioni «when»;
  - scrittura che passa dal comando `type`;
  - file system delle estensioni (`vsauto-ghost:`);
  - messaggi, scelta rapida, appunti.
- **Il cursore:** Keel lo manda all'estensione solo quando è cambiato rispetto a quello che lei conosce, e dopo ogni modifica lo aggiorna prima di rispondere, come VS Code. All'inizio lo mandava sempre e la scrittura si bloccava a caso.
- **Il test da capo a fondo** fa tutto senza finestre:
  - installa VSAuto Ghost dal VSIX e apre il file TSX di prova;
  - avvia `vsauto.start`, che apre il ghost a fianco;
  - preme 30 tasti a ritmo umano e controlla che il ghost contenga l'inizio del file originale, con l'originale intatto;
  - controlla la barra verde e il suggerimento grigio chiesto con Cmd+O;
  - ferma la sessione con Cmd+I.
- **Due comportamenti voluti di VSAuto Ghost, utili da sapere:** i tasti premuti tutti insieme vengono in parte scartati, per non accumulare arretrato, e i suggerimenti grigi compaiono a caso nelle pause.
- **Prima misura con le estensioni:** Keel in versione di sviluppo, con V8 e VSAuto Ghost caricata, occupa circa 134 MB.

**Resoconto della fase 0 (0.7)**

Misure del 15 novembre 2026, con `scripts/measure-memory.sh`, che somma la RAM di un programma e di tutti i processi che avvia:

| | RAM | Processi | Avvio |
|---|---|---|---|
| Keel, versione ottimizzata, con V8 e VSAuto Ghost caricata e un file TSX aperto | 132 MB | 1 | prima schermata in 375 ms (`KEEL_STARTUP_TIMING=1`) |
| Cursor come lo stavi usando, con la cartella Keel aperta | 1.616 MB | 11 | — |

Il confronto non è alla pari: Cursor aveva anche albero dei file, terminale, server MCP e osservatore dei file, e nessuno dei due un language server. Già la sola finestra di Cursor (`Renderer`) occupava 531 MB, quattro volte tutto Keel. Le misure si rifanno a ogni fase, a parità di condizioni, man mano che Keel aggiunge quelle parti.

**Criteri per continuare**
- Scrivere e scorrere fluido, anche con gli accenti: ✅ nei test (accenti, tasti morti, più cursori, file fino a 200.000 righe). La fluidità vera la giudichi tu.
- VSAuto Ghost come in Cursor: ✅ nel test da capo a fondo. Manca la tua prova a mano.
- RAM ben sotto Cursor: ✅ 132 MB contro 1,6 GB.

**Decisione:** si continua con la fase 1, come avevi già deciso di fare a fase 0 finita.

**Le tue prove a mano della fase 0**
1. Dalla cartella Keel compila la versione ottimizzata: `cargo build --release -p keel`.
2. Installa VSAuto Ghost: `./target/release/keel --install-extension fixtures/extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix`.
3. Apri un file: `./target/release/keel fixtures/samples/booking-card.tsx` (oppure uno dei tuoi progetti).
4. Scrivi, scorri, prova gli accenti e Cmd+Alt+↓ per più cursori; Cmd+S salva, Cmd+W chiede se ci sono modifiche.
5. Cmd+Shift+P → «Start Ghost Fake Coding». Premi tasti a caso nel ghost a destra; prova Tab sul grigio, Cmd+O, Cmd+Shift+I (pausa) e Cmd+I (stop).

**Fatto quando** (criterio per continuare)
- Scrivere e scorrere è fluido, anche con gli accenti.
- VSAuto Ghost funziona come in Cursor.
- La RAM di Keel, misurata con lo stesso script, è ben sotto quella di Cursor.

**Se non convince.** Si valuta cosa cambiare, per esempio un editor nostro sopra GPUI o QuickJS al posto di V8, oppure si ripiega su Zed più un programma esterno per VSAuto Ghost.

### Fase 1 — Fondamenta e design

**Obiettivo.** Lo scheletro definitivo dell'app e il suo aspetto, prima di riempirlo di funzioni.

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 1.1 | Bozze dell'interfaccia in `docs/bozze/index.html`, con i temi Mezzanotte, Chiaro e Nero | fatta: hai scelto B e C, unite nella bozza **D · Pannelli e Studio** |
| 1.2 | Impostazioni: `settings.json` per utente e per progetto (legge anche `.vscode/settings.json`), passate alle estensioni | fatta |
| 1.3 | Italiano e inglese: testi in una tabella per lingua, lingua predefinita quella del Mac | fatta |
| 1.4 | Comandi e scorciatoie: registro dei comandi, `keybindings.json` nel formato di VS Code, tastiera italiana | fatta; la tastiera italiana va provata a mano |
| 1.5 | Temi Mezzanotte, Chiaro e Nero, cambio al volo, segue chiaro e scuro di macOS | fatta |
| 1.6 | La disposizione della bozza D | fatta |
| 1.7 | Menu di macOS, logo e icona di Keel | fatta: menu Keel, File, Modifica e Vista; gli altri arrivano con le loro funzioni |

**Cosa si fa**
- **Bozze:** 2–3 proposte di interfaccia in stile Apple e Cursor. Scegli tu, prima di costruire.
- **Sistema di design:** colori, testi (SF Pro per l'interfaccia, SF Mono per il codice, altri font a scelta), spaziature, angoli, icone (Lucide), animazioni discrete.
- **Temi:**
  - **Mezzanotte**, predefinito, con i blu di Sail (`#070B14`, `#0B1426`, `#1E2B47`);
  - **Chiaro**;
  - **Nero**, con i neri di Sail (`#050505`, `#0A0A0A`, `#2A2A2C`).

  Keel può seguire chiaro e scuro di macOS. Il formato è compatibile con i temi di VS Code, così più avanti si importano quelli dei VSIX.
- **Disposizione della bozza D:** pannelli arrotondati sospesi sullo sfondo (come Finder e Xcode su macOS Tahoe), barra unica in alto con i semafori di macOS, barra laterale, area degli editor con il pannello in basso, Claude Code a destra, barra di stato, palette dei comandi.
- **Logo e icona di Keel**, con un marchio proprio.
- **Infrastruttura:**
  - registro dei comandi;
  - scorciatoie con le condizioni «when» nel formato di VS Code, partendo da quelle di VS Code per Mac e tenendo conto della tastiera italiana;
  - impostazioni in JSON per utente e per progetto (legge anche `.vscode/settings.json`);
  - italiano e inglese, con la scelta nelle impostazioni e come predefinita la lingua del Mac;
  - registri e gestione degli errori.
- **Menu di macOS:** Keel, File, Modifica, Selezione, Vista, Vai, Terminale, Finestra, Aiuto.

**Note sulla 1.6 e la 1.7**
- **La finestra, da sinistra a destra:**
  - **barra unica in alto**, accanto ai semafori: nome del progetto e branch git; un campo «Cerca comandi» che apre la palette; tre interruttori per barra laterale, pannello in basso e Claude Code;
  - **barra laterale**, con un selettore in cima: Esplora risorse (per ora gli editor aperti, l'albero dei file arriva nella fase 2), Cerca, Controllo del codice sorgente e Estensioni (quelle installate);
  - **editor**, con una scheda per riquadro e la X per chiuderlo. Un file chiuso resta tra gli editor aperti, così non si perde niente;
  - **pannello in basso**, nascosto all'inizio: Problemi, Output (i messaggi di Keel e delle estensioni), Terminale e Porte;
  - **Claude Code a destra**, un riquadro sospeso come gli altri. Si collega nella fase 10;
  - **barra di stato:** branch, messaggi delle estensioni, riga e colonna, rientro, codifica e linguaggio.
- **Le parti che arrivano dopo** mostrano solo «Arriva con la fase N».
- **Scorciatoie, con i nomi e i tasti di VS Code:**

  | Tasti | Cosa |
  |---|---|
  | Cmd+B | barra laterale |
  | Cmd+J | pannello in basso |
  | Cmd+Alt+B | Claude Code |
  | Cmd+Shift+E | Esplora risorse |
  | Cmd+Shift+F | Cerca |
  | Ctrl+Shift+G | Controllo del codice sorgente |
  | Cmd+Shift+X | Estensioni |
  | Cmd+Shift+M | Problemi |
  | Cmd+Shift+U | Output |
  | Ctrl+` | Terminale |

  Sono anche nel menu Vista. Sostituisci, nell'editor, passa da Cmd+Shift+F a Cmd+Alt+F, come in VS Code.
- **Per ora Keel non ricorda la disposizione:** riparte con barra laterale e Claude Code aperti. Il ripristino arriva nella fase 2, insieme a schede e file.
- **Il marchio** è una barchetta di carta piegata da un foglio di codice: la barca del nome, con i colori della sintassi di Keel sulla carta. Sta sul riquadro blu notte di Sail, sulla griglia delle icone di Apple, così Keel e Sail sembrano della stessa famiglia: Sail è la vela, Keel la barchetta. L'hai scelta il 15 novembre tra una decina di proposte; nella barra in alto c'è la sua sagoma in un solo colore.
  - `assets/brand/generate.py` contiene la forma e rifà marchio, icona e `Keel.icns`.
  - `scripts/bundle-macos.sh` crea `target/release/Keel.app` con l'icona, da usare sul Mac come le altre app. Non è firmata per altri computer: la pubblicazione resta per la fine.
- **Misure dopo la 1.6:** 132 MB con VSAuto Ghost e un file TSX aperto, prima schermata in 419 ms. Sono uguali a prima della nuova disposizione.

**Le tue prove a mano della fase 1**
1. Crea l'app: `scripts/bundle-macos.sh`, poi apri `target/release/Keel.app` dal Finder e controlla l'icona nel Dock.
2. Per aprire un file: `target/release/Keel.app/Contents/MacOS/keel fixtures/samples/booking-card.tsx`.
3. Guarda la finestra nei tre temi: Cmd+K e poi Cmd+T, oppure Keel → Tema colori.
4. Cambia lingua da Keel → Lingua e controlla menu, barra laterale, pannello e barra di stato.
5. Prova le scorciatoie della tabella con la tastiera italiana, in particolare Cmd+Shift+F e Ctrl+`. Sulla tastiera italiana non c'è il tasto `: Keel lo cerca nella stessa posizione della tastiera americana, il tasto a sinistra dell'1. Se non va, dimmelo.
6. Per cambiare un tasto, scrivilo in `~/Library/Application Support/com.manuelmezzanotte.keel/keybindings.json`, nel formato di VS Code.

**Fatto quando**
- La finestra ha la disposizione definitiva e ti piace.
- I tre temi e le due lingue si cambiano al volo.
- Le scorciatoie di base funzionano con la tastiera italiana.

### Fase 2 — Editor e file

**Obiettivo.** Lavorare sui file di un progetto come in Cursor.

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 2.1 | Progetti: `keel cartella` e `keel .`, File → Apri, una finestra per progetto, un solo Keel aperto, progetti recenti, schermata iniziale | fatta |
| 2.2 | Esplora risorse: albero con le icone, file ignorati attenuati; nuovo file e cartella, rinomina, elimina nel Cestino, sposta trascinando, mostra nel Finder, copia il percorso | fatta |
| 2.3 | Schede: più file per gruppo, anteprima, fissate, chiudi le altre, riapri l'ultima chiusa; editor affiancati e gruppi | fatta |
| 2.4 | Apertura veloce (Cmd+P) e vai alla riga (Ctrl+G) | fatta |
| 2.5 | Scrittura: commenta, sposta e duplica righe, piega i blocchi, guide di rientro, parentesi e tag chiusi da soli, a capo, zoom, breadcrumb, sticky scroll e minimappa | fatta; sticky scroll, minimappa e pieghe da tastiera rimandate alla fase 12 |
| 2.6 | Cambi dall'esterno e niente perso: ricarica i file cambiati fuori da Keel, salvataggio automatico, modifiche non salvate conservate, ripristino di schede e disposizione | fatta |
| 2.7 | Altri file (immagini, binari, file molto grandi), misure e resoconto | fatta |

**Note sulla 2.1**
- **Aprire:**
  - `keel cartella` e `keel .` aprono una cartella come progetto;
  - `keel file` apre un file nella finestra del progetto che lo contiene, oppure in una finestra nuova con la sua cartella (quella con `.git`, `.vscode` o `.keel`);
  - File → Apri… (Cmd+O) sceglie un file o una cartella.
- **Un solo Keel:** se Keel è già aperto, `keel` gli passa il percorso ed esce subito, come `code .` con VS Code. Anche aprire un file dal Finder con Keel.app passa da lì.
- **Una finestra per progetto:** riaprire un progetto già aperto porta avanti la sua finestra. Ogni finestra ha le sue impostazioni di progetto e le sue estensioni.
- **Comandi nuovi:**
  - Nuova finestra (Cmd+Shift+N);
  - Nuovo file (Cmd+N);
  - Apri recente (Ctrl+R).
- **Progetti recenti:** gli ultimi 20, in `recent.json`, nel menu File → Apri recente e nella schermata iniziale.
- **Schermata iniziale:** una finestra senza file mostra il marchio, Apri…, Nuovo file e i progetti recenti. Chiudendo l'ultimo file si torna lì. Una finestra vuota che apre un progetto lascia il posto alla finestra del progetto.
- **Estensioni:** sanno qual è la cartella del progetto (`workspace.workspaceFolders`).

**Note sulla 2.2**
- **L'albero:**
  - cartelle prima dei file;
  - nomi in ordine come nel Finder: `pagina2` prima di `pagina10`;
  - le cartelle si leggono solo quando le apri, e la lista disegna solo le righe in vista, così anche `node_modules` aperta resta veloce;
  - icone colorate per tipo di file: TypeScript, JavaScript, Rust, Python, Java, C, C++, C#, Swift, HTML, CSS, JSON, Markdown, immagini.
- **File nascosti e attenuati:**
  - nascosti come in VS Code: `.git`, `.DS_Store` e gli altri di `files.exclude`, più quelli che aggiungi nelle impostazioni;
  - attenuati: i file ignorati da git, seguendo tutti i `.gitignore`, anche quello globale.
- **Il file aperto** si vede sempre nell'albero, con le cartelle aperte fino a lui (`explorer.autoReveal`).
- **Clic:** un clic apre il file e lascia la tastiera nell'albero; il doppio clic passa all'editor.
- **Tasti come in VS Code sul Mac:**

  | Tasti | Cosa fa |
  |---|---|
  | frecce | si muovono, aprono e chiudono le cartelle |
  | Invio o F2 | rinomina |
  | Cmd+↓ o spazio | apre |
  | Cmd+Backspace | sposta nel Cestino |

- **Menu col tasto destro:**
  - nuovo file;
  - nuova cartella;
  - mostra nel Finder;
  - copia percorso;
  - copia percorso relativo;
  - rinomina;
  - sposta nel Cestino.
- **Barra dell'Esplora risorse:**
  - nuovo file;
  - nuova cartella;
  - aggiorna;
  - comprimi tutto.
- **Nomi scritti sul posto:**
  - `lib/utils.ts` crea anche le cartelle;
  - Esc annulla;
  - nella rinomina è già selezionato il nome senza estensione.
- **Spostare trascinando** chiede conferma. Eliminare sposta nel Cestino, sempre dopo una conferma: niente si cancella per sempre.
- **I file aperti seguono** rinomine e spostamenti, anche quelli dentro una cartella spostata.
- **Aggiornamento:** quando torni su Keel l'albero si rilegge, per vedere i file creati da altri, per esempio da Claude Code. Il controllo continuo arriva con la 2.6.

**Note sulla 2.3**
- **Gruppi e schede:**
  - ogni colonna dell'area degli editor è un gruppo con le sue schede;
  - le schede nascoste tengono il loro editor, con cursore, scorrimento e annullamenti;
  - se un file cambia mentre la sua scheda è nascosta, per esempio per un'estensione, la scheda si aggiorna quando torna in vista.
- **Anteprima:**
  - un clic nell'Esplora risorse apre il file in una scheda in corsivo, che lascia il posto al prossimo file aperto così;
  - scriverci dentro o fare doppio clic sulla scheda la mantiene;
  - si spegne con `workbench.editor.enablePreview`.
- **Chiudere:**
  - chiudere l'ultima scheda di un file con modifiche chiede se salvare;
  - un file non più in nessuna scheda si chiude anche per le estensioni;
  - senza schede torna la schermata iniziale.
- **Schede fissate:** stanno all'inizio, con la puntina. Chiudi gli altri, Chiudi quelli a destra e Chiudi tutti le lasciano aperte, come i file non salvati, così niente si perde senza chiedere.
- **Editor affiancati:** Dividi l'editor (Cmd+\\) apre lo stesso file in un gruppo a destra, e i due lati restano allineati mentre scrivi.
- **Scorciatoie come in VS Code sul Mac:**

  | Tasti | Cosa fa |
  |---|---|
  | Cmd+W | chiude la scheda (senza schede, la finestra) |
  | Cmd+Shift+W | chiude la finestra |
  | Cmd+Alt+T | chiude le altre |
  | Cmd+K W | chiude il gruppo |
  | Cmd+K Cmd+W | chiude tutto |
  | Cmd+Shift+T | riapre l'ultima chiusa |
  | Cmd+K Shift+Invio | fissa o sblocca |
  | Cmd+K Invio | mantiene l'anteprima |
  | Cmd+Alt+→ e Cmd+Alt+← | scheda successiva e precedente |
  | Cmd+1, Cmd+2, Cmd+3 | vai al gruppo |

- **Sulla scheda:**
  - menu col tasto destro: chiudi, chiudi gli altri, chiudi quelli a destra, chiudi tutti, fissa, dividi a destra, copia percorso, mostra nel Finder;
  - il clic con la rotella chiude la scheda;
  - un pallino segna le modifiche non salvate.
- **Per ora non si fa:** trascinare le schede per riordinarle o spostarle tra i gruppi.

**Note sulla 2.4**
- **Vai al file (Cmd+P)**, anche dal campo «Cerca file» nella barra in alto e dal menu Vai:
  - cerca tra i file del progetto, lasciando fuori quelli ignorati da git e quelli nascosti;
  - l'elenco si prepara in sottofondo all'apertura e ogni volta che torni su Keel;
  - in cima ci sono i file usati da poco;
  - accanto a ogni file c'è la sua cartella.
- **Ricerca «fuzzy» come in VS Code:**
  - le lettere devono comparire in ordine, anche separate: `bkcard` trova `booking-card.tsx`;
  - contano di più le lettere a inizio parola e quelle di seguito;
  - le lettere nel nome del file contano più di quelle nelle cartelle;
  - con una `/` la ricerca guarda il percorso.

  Ora la usano tutte le liste: palette dei comandi, progetti recenti, temi e liste delle estensioni.
- **Righe:**
  - `card.tsx:42`, e anche `:42:7`, apre il file e va alla riga, o alla riga e colonna;
  - Vai alla riga (Ctrl+G) chiede la riga, anche con `:colonna`, e mostra la riga attuale e il totale.
- **Misure su GestionaleHospitality:** 1.010 file elencati in 9 ms; ogni ricerca mentre scrivi richiede meno di 1 ms.

**Note sulla 2.5**
- **Comandi di scrittura, con i tasti di VS Code sul Mac:** funzionano solo con il cursore nell'editor, come in VS Code.

  | Tasti | Cosa fa |
  |---|---|
  | Cmd+/ | commenta la riga o le righe |
  | Shift+Alt+A | commento in blocco |
  | Alt+↑ e Alt+↓ | spostano le righe |
  | Shift+Alt+↑ e Shift+Alt+↓ | le duplicano |
  | Cmd+Shift+K | elimina le righe |
  | Cmd+Invio | nuova riga sotto, con lo stesso rientro |
  | Cmd+Shift+Invio | nuova riga sopra, con lo stesso rientro |

  Ogni comando si annulla con un solo Cmd+Z.
- **Commenti per linguaggio:**
  - `//` per TypeScript, JavaScript, Rust, C, C++, C#, Java, Swift, Go, Prisma;
  - `#` per Python, shell, YAML, TOML;
  - `--` per SQL e Lua;
  - `/* */` per CSS;
  - `<!-- -->` per HTML e Markdown.
- **Tag chiusi da soli:** in JSX/TSX e HTML, scrivendo `>` dopo `<div className="card"` compare `</div>` dopo il cursore, come in VS Code. Non succede dove `<` è un confronto o un tipo (`a < b`, `Array<string>`), dentro un attributo non chiuso, con `<br>`, `<img>` e gli altri elementi senza chiusura. Con `<>` compare `</>`.
- **Già nell'editor di gpui-component, ora con le impostazioni di Keel:**
  - parentesi e virgolette chiuse da sole;
  - rientro automatico;
  - guide di rientro;
  - pieghe dei blocchi dal margine;
  - più cursori (Cmd+Alt+↑/↓).
- **A capo automatico** (Alt+Z) vale per tutti gli editor e parte da `editor.wordWrap`.
- **Zoom:** Cmd+= e Cmd+- cambiano la dimensione del testo del codice; Vista → Dimensioni originali la riporta a `editor.fontSize`, che ora Keel rispetta.
- **Breadcrumb:** sotto le schede compare il percorso del file (`src › components › booking-card.tsx`); un clic su una cartella la mostra nell'Esplora risorse. Si spegne con `breadcrumbs.enabled`.
- **Menu nuovi:**
  - Selezione, con copia e sposta righe;
  - in Modifica, i commenti;
  - in Vista, a capo e zoom.
- **Rimandati alla fase 12:**
  - sticky scroll e minimappa: l'editor di gpui-component non li ha, vanno disegnati da Keel;
  - pieghe da tastiera: gpui-component le apre e chiude solo dal margine;
  - i comandi sulle righe con più cursori: oggi lavorano sul cursore principale.

**Note sulla 2.6**
- **Cambi dall'esterno:**
  - Keel controlla la cartella del progetto con FSEvents, il sistema di macOS;
  - quando un file cambia fuori, per esempio scritto da Claude Code, l'albero, Vai al file e il branch si aggiornano in un attimo;
  - un file aperto senza modifiche si ricarica da solo, col cursore al suo posto;
  - se avevi modifiche non salvate, Keel le tiene, avvisa nella barra di stato, e al salvataggio chiede: sovrascrivere, usare la versione sul disco o annullare.
- **Niente si perde:**
  - le modifiche non salvate si copiano in `backups/`, nella cartella dati di Keel, un secondo dopo che smetti di scrivere: tornano dopo un crash;
  - Cmd+Q non chiede niente, come in VS Code (`files.hotExit`): tiene tutto, e alla prossima apertura ritrovi finestre, schede e modifiche;
  - chiudere una finestra chiede ancora se salvare, e «Non salvare» butta via le copie.
- **Ripristino:**
  - ogni progetto ricorda gruppi, schede (anche quelle fissate), la scheda attiva, la disposizione e le cartelle aperte nell'albero, in `sessions/`;
  - aprendo Keel senza percorso tornano i progetti aperti all'uscita (`window.restoreWindows`, `none` per spegnerlo).
- **Salvataggio automatico** con `files.autoSave`, come in VS Code:
  - `afterDelay`, dopo `files.autoSaveDelay` (1000 ms);
  - `onFocusChange`;
  - `onWindowChange`.

  Spento di serie. Un file cambiato anche sul disco non si salva da solo: aspetta un salvataggio con la domanda.
- **Prove con Keel vero:**
  - 50 riscritture di un file aperto e 200 file nuovi creati mentre Keel gira: nessun errore, 130 MB;
  - avviato senza percorso, Keel riapre il progetto lasciato aperto.

**Note sulla 2.7**
- **Immagini** (PNG, JPEG, GIF, WebP, BMP, ICO, SVG, TIFF): si aprono in una scheda di anteprima, adattate allo spazio, con nome e dimensione. Le estensioni non le vedono come testo.
- **File binari:** Keel non li apre e lo dice nella barra di stato: «data.bin» non è un file di testo.
- **File oltre 50 MB:** Keel chiede prima di aprirli.

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

**Resoconto della fase 2**

Misure del 15 novembre 2026, versione ottimizzata, con `scripts/measure-memory.sh`:

| | RAM | Processi | Avvio |
|---|---|---|---|
| Keel su GestionaleHospitality, VSAuto Ghost caricata, 5 file `.tsx` in schede | 143 MB | 1 | prima schermata in 383 ms |
| Keel a fine fase 0, un file aperto | 132 MB | 1 | 375 ms |
| Cursor a fine fase 0, cartella Keel aperta | 1.616 MB | 11 | — |

Con albero, schede, ricerca dei file e controllo della cartella, Keel è cresciuto di circa 11 MB. 167 test automatici, tutti verdi.

**Le tue prove a mano della fase 2**
1. Crea l'app con `scripts/bundle-macos.sh` e, dal Terminale, nella cartella di un progetto: `~/Desktop/MiniPCProgram/Keel/target/release/Keel.app/Contents/MacOS/keel .`. Da un secondo Terminale prova `keel` su un altro file: deve aprirsi nella stessa app.
2. Esplora risorse:
   - apri cartelle e file;
   - crea, rinomina, sposta trascinando ed elimina, che va nel Cestino;
   - prova il menu col tasto destro.
3. Schede:
   - un clic apre in anteprima, il doppio clic la tiene;
   - Cmd+W, Cmd+Shift+T, fissa una scheda;
   - Cmd+\\ per affiancare.
4. Cmd+P con poche lettere, anche `nome:riga`, e Ctrl+G.
5. Scrittura:
   - Cmd+/ per commentare;
   - Alt+↑/↓ e Shift+Alt+↑/↓ per spostare e duplicare righe;
   - scrivi `<div>` in un `.tsx`;
   - Alt+Z per andare a capo, Cmd+= e Cmd+- per lo zoom.
6. Lascia che Claude Code modifichi un file aperto: deve aggiornarsi da solo. Con modifiche tue non salvate, al salvataggio Keel deve chiedere.
7. Scrivi senza salvare e premi Cmd+Q: riaprendo Keel ritrovi finestre, schede e modifiche.
8. Apri un'immagine dal progetto.

**Fatto quando**
- Apri GestionaleHospitality e ci lavori (navighi, modifichi, salvi) come in Cursor, a parte gli aiuti dei linguaggi che arrivano nella fase 6.
- Claude Code modifica un file aperto e Keel lo aggiorna subito.

### Fase 3 — Ricerca

**Obiettivo.** Trovare qualsiasi cosa nel progetto in un attimo.

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 3.1 | Motore di ricerca: maiuscole, parola intera, regex, cartelle da includere ed escludere, `.gitignore`, file binari saltati; sostituzione con i gruppi della regex | fatta |
| 3.2 | Vista Cerca nella barra laterale (Cmd+Shift+F): campi e interruttori, risultati per file, clic che porta alla riga | fatta |
| 3.3 | Sostituisci in tutti i file con l'anteprima, per file o tutto insieme | fatta |
| 3.4 | Cerca e sostituisci nel file (Cmd+F), misure e resoconto | fatta |

**Note sulla 3.4**
- **Cmd+F** apre il pannello di ricerca dell'editor di gpui-component:
  - maiuscole/minuscole;
  - successivo e precedente;
  - Sostituisci (Cmd+Alt+F, come in VS Code) e Sostituisci tutto, che si annullano con Cmd+Z.
- **Manca:** regex e parola intera dentro il singolo file, perché il pannello non le ha. Per ora si usa la vista Cerca con il file in «File da includere». Rifarle nel pannello va nella fase 12.

**Resoconto della fase 3**
- **Il criterio era** trovare una parola in GestionaleHospitality in meno di un secondo. Il motore ci mette 14–26 ms anche con migliaia di risultati, perché lavora su più thread come ripgrep.
- **La sostituzione su più file** si controlla prima nell'anteprima, chiede conferma e nei file aperti si annulla con Cmd+Z.
- **L'apertura veloce migliorata** (recenti in cima, ricerca nel percorso) era già arrivata con la 2.4.
- **Test:** 175 automatici, tutti verdi.

**Le tue prove a mano della fase 3**
1. In GestionaleHospitality, Cmd+Shift+F e una parola: i risultati devono arrivare mentre scrivi.
2. Prova gli interruttori (Aa, parola intera, `.*`) e «…» con `src` da includere.
3. Clic su un risultato: il file si apre col testo selezionato.
4. Apri Sostituisci, scrivi un testo nuovo e guarda l'anteprima, poi «Sostituisci nel file» su un file solo. Con Cmd+Z si torna indietro se il file era aperto.
5. Cmd+F e Cmd+Alt+F dentro un file.

**Note sulla 3.3**
- **La freccia** a sinistra del campo di ricerca mostra il campo Sostituisci, come in VS Code.
- **Anteprima:** ogni riga mostra il testo trovato barrato in rosso e, accanto, quello nuovo in verde, aggiornati mentre scrivi. Con una regex l'anteprima usa anche `$1`.
- **Sostituisci nel file** compare su ogni file dei risultati.
- **Sostituisci tutto** chiede prima, con quante occorrenze e quanti file.
- **File aperti e chiusi:**
  - i file aperti cambiano nell'editor: si annulla con Cmd+Z e il salvataggio resta tuo;
  - i file chiusi vengono scritti subito, in un passo solo.
- Alla fine la barra di stato dice quante sostituzioni ha fatto, e la ricerca riparte.

**Note sulla 3.2**
- **Cmd+Shift+F** apre la vista Cerca nella barra laterale e mette nel campo il testo selezionato nell'editor.
- **Quando parte:** la ricerca parte da sola dopo una breve pausa nella scrittura, oppure subito con Invio; una ricerca nuova ferma quella ancora in corso.
- **Accanto al campo**, tre interruttori come in VS Code: maiuscole/minuscole, parola intera, espressione regolare.
- **Il pulsante «…»** mostra i campi dei file da includere ed escludere.
- **Risultati:**
  - raggruppati per file, con nome, cartella e numero di risultati;
  - un clic sul file lo chiude o lo apre;
  - le righe hanno il testo trovato evidenziato;
  - un clic su una riga apre il file in anteprima con il testo trovato selezionato.
- **Riepilogo** sopra la lista, per esempio «7.310 risultati in 327 file». Una regex sbagliata mostra l'errore.

**Note sulla 3.1**
- Il motore sta nel crate `files` (`search.rs`): percorre il progetto su più thread con il crate `ignore`, come ripgrep, e cerca con il crate `regex`.
- **Lascia fuori:** i file ignorati da git, quelli nascosti dell'Esplora risorse, i binari e quelli oltre 5 MB.
- **Si ferma a 20.000 risultati**, e lo dice.
- **Cartelle come in VS Code:** «da includere» e «da escludere» accettano più modelli separati da virgola (`src, *.ts`); `src` vale per tutto quello che c'è dentro.
- **Sostituzione:** con una regex, `$1` e `${nome}` riprendono i gruppi; senza regex il testo resta letterale.
- **Misure su GestionaleHospitality**, versione ottimizzata:
  - «booking»: 7.310 risultati in 327 file in 26 ms;
  - «useState»: 594 risultati in 14 ms;
  - «z»: arriva al limite di 20.000 in 15 ms.

  Il criterio della fase era meno di un secondo.

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

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 4.1 | Motore del terminale (crate `terminal`): la shell nella cartella, colori, tasti, mouse, incolla, selezione, ricerca, link, porte | fatta |
| 4.2 | Terminale nel pannello in basso: disegno, cursore, tastiera, mouse, copia e incolla, scorrimento, ridimensionamento, Cmd+clic sui link | fatta |
| 4.3 | Più terminali (schede, divisi, rinomina, chiudi), pannello ridimensionabile, cerca nel terminale, «Apri Claude Code» | fatta |
| 4.4 | Output, Porte, script di `package.json`; misure e resoconto | fatta |

**Note sulla 4.4**
- **Porte:**
  - elenca le porte TCP su cui ascoltano i programmi partiti dai terminali di Keel, compreso quello di Claude Code;
  - guarda ogni 3 secondi, finché c'è un terminale aperto; ogni controllo costa circa 25–35 ms, fuori dalla finestra;
  - un clic sulla riga, o sul globo, apre `http://localhost:porta` nel browser;
  - una porta nuova si annuncia nella barra di stato, come la notifica di VS Code.
- **Output:** due canali, «Keel» e «Estensioni», con un pulsante per svuotare quello mostrato. I language server avranno il loro nella fase 6.
- **Problemi** resta vuoto fino alla fase 6.
- **Script npm:**
  - sotto l'albero dell'Esplora risorse c'è l'elenco degli script di `package.json`, nell'ordine del file e con il comando accanto;
  - un clic apre una scheda del terminale col nome dello script e lo lancia;
  - il gestore di pacchetti viene da `packageManager` o dal file di lock (`pnpm-lock.yaml` → pnpm, `yarn.lock` → yarn, `bun.lock` → bun, altrimenti npm);
  - c'è anche il comando «npm: Esegui uno script…».

**Resoconto della fase 4**
- **Il criterio era:**
  - `pnpm run dev` e Claude Code devono girare dentro Keel come nel Terminale di macOS: questo lo verifichi tu con le prove sotto;
  - la porta 3000 deve comparire in Porte e aprirsi con un clic: un test apre un server vero in un terminale e controlla che la porta compaia in Porte e nella barra di stato.
- **La shell è quella del Terminale di macOS:** la stessa shell di accesso, le stesse variabili per colori e lingua, e i tasti che manda VS Code sul Mac.
- **Misure** su GestionaleHospitality, versione ottimizzata:
  - con un terminale aperto, Keel usa 134 MB invece di 131 (`login` e `zsh` sono altri 16 MB, come in ogni terminale);
  - a riposo la CPU sta fra 0 e 0,6%;
  - il primo fotogramma arriva in 295 ms con il terminale riaperto dalla sessione;
  - 500.000 righe colorate attraversano il terminale in 0,82 s;
  - preparare uno schermo di 200×50 celle per il disegno prende 0,1 ms, mentre un fotogramma ne ha 16.
- **Test:** 204 automatici, tutti verdi.

**Le tue prove a mano della fase 4**
1. In GestionaleHospitality premi Ctrl+`: si apre zsh nella cartella del progetto, con i tuoi colori e il tuo prompt.
2. Lancia `pnpm run dev`, oppure clicca `dev` negli script npm sotto l'Esplora risorse. In Porte compare la 3000, e un clic la apre nel browser.
3. Prova Tab, Ctrl+R, Ctrl+C, le frecce, Opzione+frecce, Cmd+frecce e Cmd+K.
4. Seleziona col mouse e copia con Cmd+C. Incolla con Cmd+V un testo di molte righe.
5. Cmd+F cerca nel testo del terminale; Invio va più indietro.
6. Cmd+clic su un errore come `src/app/page.tsx:12:5` apre il file alla riga.
7. Apri un secondo terminale con `+`, poi dividilo; rinominalo dal clic destro sulla scheda e chiudilo con la X. Allunga il pannello trascinando il bordo in alto e prova a ingrandirlo.
8. Premi Cmd+Esc: a destra parte Claude Code. Prova a scrivere, Maiusc+Invio per andare a capo, il mouse, l'incolla di un testo lungo e Ctrl+V con un'immagine copiata. Prova anche a ridimensionare il pannello.
9. Chiudi e riapri Keel con il terminale aperto: il pannello torna con un terminale nuovo.

**Note sulla 4.3**
- **Più terminali:**
  - nell'intestazione del pannello c'è una scheda per terminale, con `+` per aprirne un altro (anche Ctrl+Maiusc+`) e il pulsante per dividere (Cmd+\ dal terminale);
  - i terminali divisi stanno uno accanto all'altro sotto la stessa scheda e partono dalla cartella di quello diviso;
  - il menu della scheda (clic destro) ha Dividi, Rinomina e Chiudi; la X chiude tutta la scheda;
  - Cmd+Maiusc+] e Cmd+Maiusc+[ passano alla scheda dopo e a quella prima.
- **Il nome della scheda** è il programma in primo piano (`zsh`, `node`, `claude`), o il titolo che un programma ha messo, o quello che gli dai con Rinomina.
- **Pannello:**
  - si allunga trascinando il bordo in alto, e l'altezza resta salvata nella sessione;
  - il pulsante accanto alla X lo ingrandisce su tutta l'area degli editor e lo riporta com'era;
  - se alla chiusura il pannello mostrava il terminale, alla riapertura ne parte uno nuovo, senza togliere la tastiera all'editor.
- **Cmd+F nel terminale** cerca nel testo, anche in quello passato:
  - parte dalle righe più recenti;
  - Invio va più indietro e Maiusc+Invio più avanti;
  - ci sono gli interruttori per maiuscole/minuscole ed espressione regolare;
  - il bordo diventa rosso quando non trova niente;
  - Esc chiude e ridà la tastiera alla shell.
- **Claude Code:**
  - Cmd+Esc, come nell'estensione di Claude Code per VS Code, lo apre a destra, nel pannello che nella fase 10 diventerà il suo pannello vero;
  - è un terminale che lancia `claude` nella cartella del progetto; quando Claude Code finisce resta la shell;
  - il pannello si allarga a 600 punti per starci comodo, e si ridimensiona trascinando il bordo sinistro;
  - «Claude Code: Apri nel pannello in basso» lo apre invece come scheda del terminale.
- **Menu Terminale** nella barra di macOS, come in VS Code, con questi comandi; ci sono anche nella tavolozza dei comandi.

**Note sulla 4.2**
- **Ctrl+`** apre il terminale nella cartella del progetto e gli dà la tastiera; se il terminale ha già la tastiera, chiude il pannello, come in VS Code. Anche la scheda «Terminale» del pannello ne apre uno se non c'è.
- **Disegno:**
  - le celle sono disegnate con la scheda video, con il carattere monospaziato del tema (o `terminal.integrated.fontFamily`), 12 punti come VS Code (o `terminal.integrated.fontSize`);
  - i colori seguono il tema, chiaro o scuro;
  - il cursore è vuoto quando il terminale non ha la tastiera;
  - le lettere larghe, come gli emoji, restano allineate alla griglia.
- **Tasti che restano alla shell:**
  - Tab non sposta più la tastiera altrove;
  - Ctrl+R cerca nella cronologia della shell e Ctrl+G non apre «Vai alla riga»;
  - Cmd+K pulisce il terminale subito, senza aspettare gli accordi come Cmd+K Cmd+T;
  - Ctrl+` e Ctrl+Maiusc+G funzionano anche dal terminale, come in VS Code.
- **Copia e incolla:**
  - Cmd+C copia la selezione, Cmd+A seleziona tutto;
  - Cmd+V incolla il testo; se negli appunti c'è solo un'immagine, Keel manda Ctrl+V, che è il modo in cui Claude Code legge le immagini.
- **Mouse:**
  - trascinare seleziona, doppio clic prende la parola e triplo clic la riga;
  - con i programmi che usano il mouse, come Claude Code, il mouse va a loro, e Maiusc lo tiene per selezionare.
- **Scorrimento:** la rotella scorre le righe passate, fino a 10.000; nei programmi a schermo intero, come `less`, manda le frecce. Maiusc+Pag su e Maiusc+Pag giù scorrono di una pagina.
- **Link:** Cmd+clic su un indirizzo lo apre nel browser; Cmd+clic su un percorso come `src/app/page.tsx:12:5` apre il file alla riga, cercandolo dalla cartella in cui si trova il terminale.
- **Fine della shell:** con `exit` il terminale si chiude, e con l'ultimo si chiude il pannello.

**Note sulla 4.1**
- **La shell** è la tua, aperta come fa il Terminale di macOS: `login` con zsh come shell di accesso, quindi legge `.zprofile` e `.zshrc` e ha il PATH giusto anche se Keel parte dal Finder.
- **Variabili:** `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=Keel`; `LANG` dalla lingua del Mac quando manca, altrimenti zsh mostra le lettere accentate come codici.
- **Tasti come in VS Code sul Mac:**
  - Opzione+lettera scrive il carattere di macOS (`å`);
  - Opzione+frecce va di parola in parola;
  - Cmd+frecce va a inizio e fine riga, Cmd+Backspace cancella la riga;
  - Maiusc+Invio va a capo in Claude Code, come fa il suo `/terminal-setup`.
- **Mouse e incolla:** il mouse va ai programmi che lo chiedono (Claude Code, vim, htop). Il testo incollato arriva in un pezzo solo ai programmi che lo chiedono, così un testo lungo non parte riga per riga.
- **Appunti:** i programmi possono copiare negli appunti, ma non leggerli, così un comando non può prendere una password copiata poco prima.
- **Link:** trova indirizzi web e percorsi come `src/app/page.tsx:12:5` o `a.ts(3,9)`.
- **Porte:** quelle dei processi partiti dai terminali, lette con `ps` e `lsof`.
- **La cartella del programma in primo piano** segue i `cd`, letta da macOS. Serve per i link e per i nuovi terminali.

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

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 5.1 | Motore git (crate `git`): stato, differenze, stage e unstage anche di un blocco, scarta, commit e amend, branch, push, pull, fetch, sync, stash | fatta |
| 5.2 | Vista Controllo del codice sorgente: messaggio e Commit, file in stage, modifiche e conflitti, numero sull'icona, branch nella barra di stato | fatta |
| 5.3 | Differenze in una scheda, affiancate o in linea; conflitti di merge risolti con un clic | fatta |
| 5.4 | Margine dell'editor (aggiunte, modifiche, cancellazioni) con stage e annulla del blocco; push, pull, sync, branch e stash dai comandi | fatta |
| 5.5 | Misure e resoconto | fatta |

**Resoconto della fase 5**
- **Il criterio era un giro completo solo da Keel:** modifichi, controlli le differenze, metti in stage, fai il commit e il push. I test automatici fanno questo giro in un repository vero, con un remoto creato apposta: stage di file e di un blocco, commit da Cmd+Invio, amend, push con pubblicazione del branch, pull dopo un push fatto da un'altra copia, stash, cambio di branch, risoluzione di un conflitto di merge. Resta la tua prova con GitHub.
- **Un problema trovato con le misure:** con la vista git aperta Keel usava l'1,8% di CPU senza fare niente, perché il campo del messaggio di gpui-component, quando cresce da solo (`auto_grow`), si ridisegna a ogni fotogramma. Ora la sua altezza la calcola Keel contando le righe, e la CPU a riposo è scesa allo 0,1%.
- **Misure** su GestionaleHospitality, versione ottimizzata, con la vista git aperta:
  - 126 MB di memoria, primo fotogramma in 323 ms;
  - lettura dello stato di git: 17 ms, fatta in background;
  - blocchi cambiati di un file di 20.000 righe con 424 modifiche: 1,3 ms;
  - a riposo Keel lancia git solo quando serve: 4 comandi in 15 secondi all'avvio, poi niente.
- **Test:** 216 automatici, tutti verdi.

**Le tue prove a mano della fase 5**
1. In un tuo progetto modifica due file e creane uno nuovo. Sull'icona della vista git compare 3 e nel margine dell'editor i segni verdi e azzurri.
2. Metti il cursore su un blocco cambiato: compare la barra con Differenze, Metti in stage e Annulla. Prova anche Alt+F5.
3. Apri la vista git (Ctrl+Maiusc+G) e clicca un file: si apre la scheda delle differenze. Prova «Affiancate» e «In linea».
4. Metti in stage un file col `+`, scrivi il messaggio e premi Cmd+Invio.
5. Clicca il branch nella barra di stato: creane uno nuovo, poi torna su `develop`.
6. Dal menu «…» fai Push (o «Pubblica il branch» su quello nuovo) e controlla su GitHub.
7. Scarta una modifica col pulsante a freccia: chiede conferma. Un file nuovo scartato va nel Cestino.
8. Se ti capita un conflitto di merge, aprilo dalla vista: scegli per ogni conflitto e premi «Salva e metti in stage».

**Note sulla 5.4**
- **Il margine dell'editor** confronta il testo, anche non salvato, con la versione in stage, come VS Code:
  - verde per le righe aggiunte;
  - azzurro per quelle cambiate;
  - rosso dove sono state tolte delle righe.

  I file che git non conosce non hanno segni. Se un'estensione offre il suo confronto (come VSAuto Ghost), vale il suo.
- **Il clic sul margine** di gpui-component non si può intercettare, quindi l'anteprima di VS Code è diventata una piccola barra. Compare in alto a destra dell'editor quando il cursore è in un blocco cambiato, con tre pulsanti:
  - **Differenze** apre la scheda delle modifiche già sul blocco;
  - **Metti in stage** mette in stage solo quel blocco;
  - **Annulla** rimette il blocco com'era, e si torna indietro con Cmd+Z.
- **Alt+F5 e Maiusc+Alt+F5** vanno al blocco dopo e a quello prima, come in VS Code.
- **Nella tavolozza** ci sono anche «Git: Metti in stage il blocco» e «Git: Annulla il blocco».
- **Push, pull, fetch, sincronizza, branch e stash** sono arrivati con la 5.2, nel menu della vista e nella tavolozza.

**Note sulla 5.3**
- **Un clic su un file** della vista apre le sue differenze in una scheda di anteprima, come in VS Code:
  - «nome (modifiche)» confronta la versione in stage con il file sul disco;
  - «nome (in stage)» confronta l'ultimo commit con la versione in stage.
- **La vista delle differenze** è quella di gpui-component:
  - colori della sintassi, numeri di riga e righe uguali ripiegate;
  - «Affiancate» e «In linea» si scelgono dalla barra della scheda, e la scelta vale per tutte le schede; il primo valore viene da `diffEditor.renderSideBySide`, come in VS Code;
  - nella barra ci sono anche «Metti in stage» o «Togli dallo stage» e «Apri il file».
- **Le schede aperte seguono git:** dopo uno stage, un commit o un salvataggio le differenze si rileggono da sole.
- **Conflitti di merge:**
  - un file in conflitto si apre con un pulsante per ogni conflitto: tieni la versione corrente, quella in arrivo o entrambe;
  - quando sono risolti tutti, «Salva e metti in stage» scrive il file, lo mette in stage e lo riapre nell'editor.

**Note sulla 5.2**
- **La vista** (Ctrl+Maiusc+G) ha in alto il messaggio, che cresce fino a 8 righe, e il pulsante «Commit su «branch»». Cmd+Invio nel messaggio fa il commit.
- **Sotto ci sono tre elenchi:** Conflitti di merge, Modifiche in stage e Modifiche. Ogni file mostra il nome, la cartella e la lettera colorata (M, A, D, U, R). Passandoci sopra compaiono i pulsanti:
  - apri il file;
  - scarta le modifiche;
  - metti in stage o togli dallo stage.

  Sui titoli degli elenchi ci sono gli stessi pulsanti per tutti i file insieme.
- **Commit senza niente in stage:** chiede se mettere tutto in stage e fare il commit, come lo «smart commit» di VS Code.
- **Scarta:**
  - chiede sempre conferma;
  - i file che git conosce tornano come erano;
  - i file nuovi vanno nel Cestino, così si possono recuperare.
- **Il menu «…»** ha Pull, Push, Fetch, Sincronizza, Commit (amend), Cambia branch, Crea un branch, Metti da parte e Riprendi le modifiche. Gli stessi comandi sono nella tavolozza, con i nomi di VS Code (`git.push`, `git.checkout`…).
- **Senza modifiche,** se il branch è avanti o indietro rispetto al remoto, compare «Sincronizza ↓1 ↑2»; se non è ancora sul remoto, «Pubblica il branch».
- **Sull'icona della vista** c'è il numero dei file cambiati.
- **Nella barra di stato:**
  - un clic sul branch apre l'elenco per cambiarlo o crearne uno;
  - accanto al branch compare `↓1 ↑2`, e un clic sincronizza;
  - mentre git lavora c'è scritto, per esempio, «Push…».
- **Quando si aggiorna:** lo stato si rilegge in background quando cambia un file, quando salvi, quando torni su Keel e quando git cambia l'indice o i branch, anche con un comando lanciato nel terminale.
- **Gli errori di git** (per esempio un push rifiutato) vanno nella barra di stato e nell'Output.

**Note sulla 5.1**
- **Git passa dal comando `git`**, invece che da `gix` come diceva lo stack: così Keel usa lo stesso programma, le stesse impostazioni, gli stessi hook e le stesse credenziali del terminale, con una dipendenza grossa in meno.
- **Velocità:** `git status` prende circa 16 ms su GestionaleHospitality.
- **Lo stato si legge senza bloccare l'indice**, così non disturba i comandi git lanciati nel terminale.
- **I blocchi cambiati** si trovano con `imara-diff`, lo stesso algoritmo «histogram» di git.
- **Stage di un blocco solo:** Keel scrive nell'indice il file con quel blocco cambiato e lascia stare il file sul disco.
- **Push:** un branch senza upstream si pubblica sul remote `origin` (o sul primo che c'è) e da lì in poi lo segue. Se servisse una password, il push fallisce con un messaggio invece di restare in attesa; il Portachiavi di macOS risponde come sempre.

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

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 6.1 | Colori della sintassi per JavaScript, HTML, CSS, JSON, Markdown, YAML, TOML, Rust e `.env` (oggi solo TypeScript) | fatta |
| 6.2 | Motore LSP (crate `lsp`): processo, messaggi, documenti aperti, richieste del server, riavvio dopo un crash, registro nell'Output; trova o installa i server | fatta |
| 6.3 | Errori e avvisi: sottolineature, pannello Problemi, numeri nella barra di stato, F8 per passare al successivo | fatta |
| 6.4 | Scrivere: completamenti con snippet, info al passaggio del mouse, firma delle funzioni, snippet nel formato di VS Code | fatta |
| 6.5 | Navigare: definizione, implementazione e tipo, riferimenti, rinomina, struttura del file (Cmd+Maiusc+O), simboli del progetto (Cmd+T) | fatta |
| 6.6 | Correggere e formattare: azioni rapide (Cmd+.), formattazione anche al salvataggio, Prettier del progetto, correzioni di ESLint | fatta |
| 6.7 | I server: TypeScript (`vtsls`), HTML, CSS, JSON, ESLint, Emmet, Tailwind, Prisma, Markdown, YAML, TOML e `rust-analyzer`; prove sui due gestionali e su Keel | fatta |
| 6.8 | Misure e resoconto | fatta |

**Resoconto della fase 6**
- **Il criterio era:** su GestionaleHospitality e GestionaleSalon Keel mostra gli stessi errori di Cursor, completa, formatta al salvataggio, rinomina e trova i riferimenti; su Keel stesso `rust-analyzer` funziona.
  - **Gli stessi errori:** provato con un errore aggiunto in memoria su un file di ciascun progetto. TypeScript dà lo stesso errore di `tsc`, ESLint lo stesso avviso di `eslint` (vedi la 6.7).
  - **Completamenti, rinomina, riferimenti, formattazione:** funzionano coi server veri (tempi nella 6.7) e nei test automatici, che fanno il giro completo nella finestra con un server finto.
  - **rust-analyzer** funziona su Keel: errori, info, definizioni, struttura e formattazione.
  - **Resta la tua prova a mano.**
- **Misure** con Keel ottimizzato, aprendo un file dal comando `keel`:

  | | Keel | Server | Primo fotogramma | CPU di Keel a riposo |
  |---|---|---|---|---|
  | Hospitality, `sidebar.tsx` (TypeScript, ESLint, Tailwind, Emmet) | 140 MB | 1,18 GB in 7 processi | 567 ms | 0,1% |
  | Keel, `text.rs` (rust-analyzer) | 98 MB | 2,2 GB mentre indicizza, poi circa 600 MB | 431 ms | 0,1% |

  - **I server pesano più di Keel.** Sono gli stessi programmi che usa Cursor: il solo server di TypeScript prende 587 MB su Hospitality, e quello di Cursor in questo momento ne prende 1,2 GB su un altro tuo progetto. Keel aggiunge 140 MB; la sola finestra di Cursor ne prende più di 600.
  - Con la cartella dati nuova i server si installano da soli la prima volta: da 20 a 30 secondi per quelli di npm. Le misure sono state fatte dopo.
  - **Chiudendo Keel i server si fermano tutti**, anche quelli che avviano altri processi, come il `tsserver` di TypeScript.
- **Rimandati:**
  - i suggerimenti in linea, che l'editor di gpui-component non sa disegnare (fase 12);
  - la lampadina delle azioni rapide accanto alla riga;
  - il riquadro dei riferimenti dentro l'editor: per ora c'è un elenco.
- **Test:** 273 automatici, tutti verdi, più 11 da lanciare a mano: misure e server veri (`cargo test -p lsp --test real_servers -- --ignored`).

**Le tue prove a mano della fase 6**

Apri Keel su GestionaleHospitality. La prima volta i server si installano da soli: nella barra di stato compare «Installo il server…», e la prima volta TypeScript ci mette qualche secondo.
1. Apri un file `.tsx` e scrivi un errore, per esempio `const x: number = "a";`. Si sottolinea in rosso. Nella barra di stato compaiono gli errori e gli avvisi; un clic apre Problemi. F8 va al problema successivo.
2. Scrivi `use` dentro un componente: compare il menu dei completamenti. Scegli `useState` da un file che non lo importa ancora: la riga di `import` si aggiunge da sola in cima.
3. Scrivi il nome di una funzione e `(`: sopra il cursore compare la firma, con il parametro in grassetto. Scrivi `,` e il grassetto passa al parametro dopo.
4. Passa col mouse su un nome: compare il tipo. Cmd+clic, o F12, apre la definizione, anche in un altro file. Maiusc+F12 elenca i riferimenti.
5. F2 su una variabile usata in più file: rinominala e controlla i file, anche quelli chiusi (con la vista git).
6. Cmd+. sopra un errore: compaiono le correzioni. Maiusc+Alt+O ordina gli import.
7. Maiusc+Alt+F formatta il file. Per formattare al salvataggio aggiungi `"editor.formatOnSave": true` alle impostazioni e salva con Cmd+S.
8. Cmd+Maiusc+O elenca i simboli del file; Cmd+T cerca i simboli del progetto.
9. Apri `prisma/schema.prisma`, `globals.css`, `package.json` e `compose.yaml`: errori e completamenti anche lì.
10. Apri Keel su sé stesso e un file `.rs`: nella barra di stato si vede l'indicizzazione di rust-analyzer, poi errori, info e F12 funzionano.
11. Snippet: copia i tuoi snippet di Cursor nella cartella `snippets` di Keel e prova un prefisso; Tab passa al segnaposto dopo.
12. Nell'Output, canale «Linguaggi», ci sono i messaggi dei server. «Linguaggi: Riavvia i language server» dalla tavolozza li riavvia.

**Note sulla 6.7**
- **Ogni server è stato provato su un file vero.** In memoria, senza toccare il disco, ho aggiunto un errore e confrontato con gli strumenti a riga di comando dei progetti (tempi della prima volta, con il server appena partito):

  | Progetto e file | Server | Primo errore | Uguale a |
  |---|---|---|---|
  | Hospitality, `sidebar.tsx` | TypeScript | 2,5 s | `tsc` |
  | Hospitality, `sidebar.tsx` | ESLint | 1,7 s | `eslint` |
  | Hospitality, `sidebar.tsx` | Tailwind | 2,3 s | suggerimenti sulle classi |
  | Hospitality, `schema.prisma` | Prisma | 0,7 s | — |
  | Hospitality, `globals.css` | CSS | 0,2 s | — |
  | Hospitality, `package.json` | JSON | 0,4 s | — |
  | Hospitality, `compose.yaml` | YAML | 0,4 s | — |
  | Salon, `brand.tsx` | TypeScript ed ESLint | 3,0 s e 1,5 s | `tsc` ed `eslint` |
  | Keel, `Cargo.toml` | TOML | 0,5 s | — |
  | Keel, `text.rs` | rust-analyzer | 5,7 s, mentre indicizza | `cargo check` |

  Con il server già avviato le risposte arrivano in millisecondi: info 8 ms, definizione 2 ms, struttura del file 12 ms, formattazione 14 ms. Un completamento di TypeScript costa circa 140 ms; il primo di un file circa 0,9 s.
- **Cosa ho dovuto sistemare perché funzionassero come in VS Code:**
  - **ESLint** vuole le sue impostazioni nella forma dell'estensione di VS Code. Prima di controllare aspetta il messaggio «impostazioni cambiate», e poi vuole che sia l'editor a chiedere gli errori («pull»). Keel ora fa entrambe le cose, anche per gli altri server che le usano.
  - **CSS, HTML e JSON** formattano solo se l'editor lo chiede all'avvio; JSON scarica da solo gli schemi di `package.json`, `tsconfig.json` e degli altri file noti.
  - **Prisma** si bloccava se le sue impostazioni mancavano: ora ha le sue predefinite, e ogni sezione mai impostata arriva come oggetto vuoto, come in VS Code.
  - **TOML** ci metteva 10 secondi ad avviarsi cercando un catalogo di schemi che non risponde; ora usa quello di SchemaStore e parte in 0,16 s.
  - **TypeScript** con la parola ancora vuota mandava 15.000 completamenti in un secondo; ora ne manda al massimo 1.000, già filtrati.
  - **rust-analyzer**, mentre indicizza, risponde «contenuto cambiato»: Keel riprova da solo, come VS Code.
  - **Nei progetti con Tailwind** il server CSS non segnala le regole `@theme`, `@apply` e simili, che non sono CSS standard.
- **Più di Cursor:** in Cursor non hai le estensioni di ESLint, Tailwind e Prisma, quindi lì vedi solo gli errori di TypeScript. Keel avvia anche quei server, perché i progetti li usano. Si spengono con `"keel.languageServers.disabled": ["eslint", "tailwindcss"]`.
- **Emmet** funziona in HTML, CSS e JSX. **Markdown** dà la struttura del file, i link e i riferimenti; non segnala errori se i link sono giusti.

**Note sulla 6.6**
- **Azioni rapide (Cmd+.)**, anche dal menu del tasto destro:
  - le correzioni dei problemi sotto il cursore e i refactoring che offre il server, prima quelle consigliate;
  - se un'azione arriva senza modifiche, Keel le chiede al server al momento di applicarla;
  - un'azione può modificare più file o lanciare un comando del server.

  VS Code mostra anche una lampadina accanto alla riga; per ora Keel non la disegna.
- **Organizza gli import:** Maiusc+Alt+O.
- **Formatta il documento** (Maiusc+Alt+F):
  - se il progetto ha Prettier in `node_modules`, usa quello, con le sue regole e il suo `.prettierignore`;
  - altrimenti formatta il language server, con la tabulazione delle impostazioni;
  - `editor.defaultFormatter` decide come in VS Code: con `esbenp.prettier-vscode` vale Prettier, con un altro valore il server.
- **Al salvataggio con Cmd+S**, nell'ordine:
  1. le azioni di `editor.codeActionsOnSave` (per esempio `source.fixAll.eslint` o `source.organizeImports`);
  2. la formattazione, se `editor.formatOnSave` è acceso;
  3. il salvataggio.

  Le impostazioni valgono anche per un solo linguaggio, con `"[typescript]": { ... }`, come in VS Code. Il salvataggio automatico non formatta, come in VS Code. Se tutto il lavoro supera 3 secondi, Keel salva lo stesso e lo scrive nell'Output.
- **ESLint:** le sue correzioni arrivano dal suo server nelle azioni rapide e con `source.fixAll.eslint` al salvataggio. Il server ESLint si sistema nella 6.7.

**Note sulla 6.5**
- **Vai alla definizione:**
  - F12, oppure Cmd+clic sul nome: Cmd sottolinea il nome sotto il mouse, come in VS Code;
  - si apre anche un altro file, già sul punto giusto;
  - se le definizioni sono più di una, compare un elenco da cui scegliere.
- **Altri salti:**
  - Cmd+F12 va alle implementazioni;
  - «Vai alla definizione del tipo» è nella tavolozza.
- **Riferimenti (Maiusc+F12):** un elenco con la riga di codice e `file:riga` di ciascun riferimento. Si filtra scrivendo, Invio apre il punto. VS Code li mostra in un riquadro dentro l'editor; per ora Keel usa l'elenco.
- **Rinomina (F2):**
  - chiede il nuovo nome con quello attuale già selezionato;
  - cambia tutti i file del progetto;
  - i file aperti cambiano nell'editor, dove Cmd+Z torna indietro; quelli chiusi cambiano sul disco;
  - la barra di stato dice quante modifiche in quanti file.
- **Struttura del file (Cmd+Maiusc+O):** l'elenco dei simboli del file, in ordine e rientrati come sono annidati, con il tipo (funzione, classe…).
- **Simboli del progetto (Cmd+T):** si cercano nel server mentre scrivi, con il tipo e il file.
- **Modifiche chieste dai server:** alcune azioni fanno modificare al server altri file, oppure creare, spostare o eliminare file (eliminati vuol dire nel Cestino). Keel le applica tutte.
- **I server sanno cosa cambia sul disco.** Quando un file cambia fuori dall'editor (un `git checkout`, un file scritto da Claude Code, una rinomina su file chiusi), Keel avvisa i server che lo hanno chiesto. Così TypeScript non lavora su versioni vecchie.

**Note sulla 6.4**
- **Completamenti** dai server, dalle estensioni e dagli snippet, in un solo menu:
  - compaiono mentre scrivi una parola e dopo i caratteri del server, come `.`; Ctrl+Spazio li chiede a mano;
  - le lettere scritte devono comparire in ordine nel nome (`ust` trova `useState`), e prima vengono i nomi che cominciano così;
  - le lettere trovate sono evidenziate.
- **Import automatici:** scegliendo un nome da un altro modulo, Keel aggiunge la riga di `import` in cima. Se il server la dà solo su richiesta, come TypeScript, la chiede subito dopo. Il cursore resta dov'era.
- **Come Keel capisce cosa hai scelto.** gpui-component inserisce da solo la voce del menu, ma non dice quale. Keel la riconosce dal testo inserito. Se più voci inseriscono lo stesso testo (per esempio `Button` da due moduli), segue le frecce che hai premuto nel menu; scegliendo col mouse fra voci uguali vale la prima.
- **Snippet:**
  - i completamenti con segnaposto (`console.log($1)`) mettono il cursore al primo posto, con il testo predefinito selezionato;
  - Tab passa al posto dopo, Maiusc+Tab torna indietro, Esc esce;
  - variabili come `TM_FILENAME` e `CURRENT_YEAR` si riempiono da sole.
- **I tuoi snippet:** si leggono nel formato di VS Code dalla cartella `snippets` di Keel: `typescript.json` per un linguaggio, oppure `*.code-snippets` con `scope`. Si possono copiare da quella di Cursor. Valgono anche gli snippet delle estensioni installate.
- **Info al passaggio del mouse:** tipo e documentazione dal server. Sopra un errore compare il suo messaggio.
- **Firma delle funzioni:** dopo `(` o `,` compare sopra il cursore la firma, con il parametro che stai scrivendo in grassetto, e segue il cursore. Dopo il completamento di una funzione compare da sola; Cmd+Maiusc+Spazio la chiede a mano, Esc la chiude.
- **Velocità:** 2.000 voci di un server si preparano in 1,6 ms. A ogni tasto il server riceve prima la modifica e poi la richiesta, quindi lavora sempre sul testo giusto.
- **Rimandati:** i suggerimenti in linea (i tipi scritti in grigio dentro il codice) vogliono del testo «virtuale» che l'editor di gpui-component non sa disegnare. Restano per la fase 12. Il tipo si legge intanto passando col mouse.

**Note sulla 6.3**
- **Sottolineature:** rosse per gli errori, gialle per gli avvisi, blu per le informazioni. Passandoci sopra col mouse compare il messaggio.
- **Le sottolineature seguono il testo.** gpui-component le cancella a ogni tasto, finché il server non risponde: lampeggerebbero mentre scrivi. Keel invece le sposta insieme al testo, come VS Code, e le sostituisce quando arrivano quelle nuove.
- **I suggerimenti** (per esempio una variabile non usata) restano fuori dalle sottolineature, dal pannello e dai numeri, come in VS Code.
- **Pannello Problemi** (Cmd+Maiusc+M):
  - i file con i loro problemi, dal più grave, ciascuno con la fonte, il codice e la posizione;
  - un clic apre il file sul punto;
  - un clic sul file lo chiude o lo riapre;
  - il numero dei problemi compare anche sulla scheda del pannello.
- **Barra di stato:** subito dopo il branch, il numero di errori e di avvisi. Un clic apre il pannello Problemi.
- **F8 e Maiusc+F8** portano al problema dopo o prima del cursore, anche negli altri file, e ne mostrano il messaggio nella barra di stato.
- **Esplora risorse:** i file con errori sono rossi, quelli con soli avvisi gialli, con il numero dei problemi accanto. Le cartelle che li contengono prendono il colore del più grave.

**Note sulla 6.2**
- **Il crate `lsp`** parla con i server: avvia il processo, manda e riceve i messaggi, annulla le richieste che non servono più e risponde da solo alle domande di routine (impostazioni, cartelle del progetto).
- **Le modifiche viaggiano piccole:** a ogni tasto Keel manda al server solo il pezzo cambiato, non tutto il file.
- **I server partono da soli** quando apri un file del loro linguaggio, e si fermano con la finestra. Keel li cerca in quest'ordine:
  - nella sua cartella `servers`;
  - per `rust-analyzer`, nella toolchain del progetto;
  - nel `PATH` della tua shell.

  Il `PATH` lo legge dalla shell di login (0,4 secondi, una volta sola, fuori dalla finestra). Così trova Node, Homebrew e `~/.cargo/bin` anche quando apri Keel dal Finder.
- **Se un server manca, lo installa:**
  - i server di npm in una cartella sua, con la versione bloccata (20–30 secondi la prima volta);
  - `rust-analyzer` con `rustup component add`;
  - Markdown e TOML scaricando il programma dalla pagina delle versioni. Il TOML di npm non ha il language server dentro.

  Mentre installa lo scrive nella barra di stato. L'impostazione `keel.languageServers.autoInstall` spegne l'installazione automatica; `keel.languageServers.disabled` spegne dei server per nome, per esempio `["eslint"]`.
- **ESLint e Tailwind** partono solo nei progetti che li usano: un file di configurazione o il pacchetto in `package.json`.
- **Se un server si blocca, ripartono da soli,** fino a 4 volte in 3 minuti, come in VS Code. Poi Keel lo lascia fermo e lo dice. «Linguaggi: Riavvia i language server», nella tavolozza, li riavvia tutti.
- **Messaggi dei server:** vanno nell'Output, canale «Linguaggi», con il nome del server davanti. Il lavoro in corso, come l'indicizzazione di rust-analyzer, compare nella barra di stato.
- **Provati con i server veri** in una cartella di prova:

  | Server | Avvio | Primi errori |
  |---|---|---|
  | TypeScript (`vtsls`) | 0,2 s | 0,8 s |
  | JSON | 0,1 s | 0,6 s |
  | CSS | 0,1 s | 0,6 s |
  | YAML | 0,2 s | 0,4 s |
  | rust-analyzer | 0,5 s | 2,6 s |

  Il server TOML per ora ci mette 10 secondi ad avviarsi: lo sistemo nella 6.7.

**Note sulla 6.1**
- **Colori per:** JavaScript e JSX (con la grammatica di TSX), JSON, HTML (con JavaScript e CSS dentro), CSS, SCSS e Less, Markdown (con i blocchi di codice colorati nel loro linguaggio), YAML, TOML, Rust, script della shell e file `.env`.
- **Il linguaggio si sceglie anche dal nome del file:**
  - `tsconfig.json` e le impostazioni di VS Code sono JSON con commenti;
  - `.env`, `.env.local` e simili sono file `.env`;
  - `Cargo.lock` è TOML.
- **Prisma** non ha una grammatica in gpui-component: per ora resta senza colori. Lo segue comunque il suo language server nella 6.7.
- **Velocità:** un file Rust di 24.000 righe si analizza in 119 ms, in background; la prima schermata si colora in meno di un millisecondo.

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

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 7.1 | Anteprima Markdown in una scheda, affiancata (Cmd+K V) o al posto dell'editor (Cmd+Maiusc+V), aggiornata mentre scrivi, con i colori del tema | fatta |
| 7.2 | Scorrimento sincronizzato nei due versi; link ad altri file, a titoli e al web; immagini locali; liste di controllo da spuntare | fatta |
| 7.3 | Immagini e SVG con zoom | fatta |
| 7.4 | PDF in una scheda con PDFKit: pagine, zoom, miniature, ricerca, copia del testo | fatta |
| 7.5 | Misure e resoconto | fatta |

**Resoconto della fase 7**
- **Il criterio era:** i tuoi `.md` si leggono almeno bene come in Cursor e i PDF si aprono dentro Keel.
  - In Cursor usi Markdown Preview Enhanced. Keel ha l'anteprima affiancata che segue il testo, lo scorrimento nei due versi, le tabelle, le liste di controllo da spuntare, il codice colorato, le immagini e i link. Mancano i diagrammi Mermaid e le formule disegnate: restano nelle fasi opzionali, con i pannelli web.
  - I PDF si aprono in una scheda, con miniature, zoom, ricerca e copia del testo.
  - **Resta la tua prova a mano.**
- **Misure:**

  | | Risultato |
  |---|---|
  | Blocchi di `docs/PIANO.md` (1.303 righe), per lo scorrimento sincronizzato | 16 ms, al primo scorrimento dopo una modifica |
  | Specifica OpenGL 4.4, 953 pagine: apertura | 83 ms |
  | Una pagina disegnata a 1.600 pixel | da 10 a 86 ms, nel thread del PDF |
  | Ricerca di «texture» in tutto il documento (3.590 risultati) | 1,5 s, nel thread del PDF |
  | Keel ottimizzato con quel PDF aperto | 190 MB, primo fotogramma in 577 ms, CPU a riposo 0,05% |

- **Test:** 286 automatici, tutti verdi, più 13 da lanciare a mano (misure e server veri).

**Le tue prove a mano della fase 7**
1. Apri un tuo `.md` (per esempio `docs/PIANO.md` di Keel) e premi Cmd+K V: l'anteprima si apre a lato. Scrivi nell'editor e guarda l'anteprima cambiare.
2. Scorri l'editor e poi l'anteprima: l'altra parte segue.
3. Nell'anteprima clicca un link a un titolo (`#…`), uno a un altro file e uno a una pagina web.
4. Spunta una casella di una lista di controllo: nel testo `[ ]` diventa `[x]`.
5. Prova Cmd+Maiusc+V, che apre l'anteprima al posto dell'editor, e il pulsante in fondo alla fila delle schede.
6. Apri un'immagine e un SVG: clic, Alt+clic, Cmd con la rotella, Cmd+0. Nella barra di stato ci sono dimensioni e zoom.
7. Apri un PDF: scorri, apri le miniature, cambia lo zoom, cerca una parola con Cmd+F e Invio.
8. Trascina sul testo del PDF, premi Cmd+C e incolla in un editor.

**Note sulla 7.4**
- **I PDF si aprono in una scheda,** dall'Esplora risorse, da Cmd+P o dal comando `keel`.
- **Li disegna PDFKit di macOS** (crate `pdf`, con `objc2-pdf-kit`): ogni pagina diventa un'immagine della larghezza giusta per lo schermo Retina. PDFKit lavora in un thread suo, quindi la finestra non aspetta mai una pagina.
- **Pagine:**
  - una sotto l'altra, su sfondo grigio, adattate alla larghezza;
  - si disegnano solo quelle che si vedono, e ne restano in memoria al massimo 12: le altre si ridisegnano quando tornano in vista;
  - mentre cambia lo zoom resta l'immagine vecchia finché arriva quella nuova.
- **Barra in alto:**
  - miniature, in una colonna a sinistra: un clic va alla pagina;
  - «Pagina 3 di 12», anche nella barra di stato;
  - zoom −, «Adatta», + (anche Cmd+=, Cmd+- e Cmd+0, con gli stessi livelli delle immagini);
  - ricerca (Cmd+F): i risultati sono evidenziati sulle pagine, Invio e Maiusc+Invio passano al successivo e al precedente, senza badare a maiuscole e accenti.
- **Copiare il testo:** trascinando sulla pagina si seleziona il testo come in Anteprima, anche fra due pagine; Cmd+C lo copia.
- **Non c'è ancora:** le pagine ruotate di 90° si disegnano, ma ricerca e selezione lì possono cadere nel punto sbagliato; i PDF protetti da password non si aprono; i link dentro il PDF non si seguono.

**Note sulla 7.3**
- **Zoom delle immagini e degli SVG,** come nell'anteprima di VS Code:
  - all'apertura l'immagine si adatta allo spazio;
  - un clic ingrandisce, Alt+clic rimpicciolisce;
  - Cmd con la rotella o il trackpad fa lo stesso;
  - Cmd+= e Cmd+- vanno ai livelli dopo e prima (10%, 20%… 100%, 150%, 200%… fino a 2000%), Cmd+0 torna ad «Adatta»;
  - ingrandita oltre lo spazio, l'immagine scorre.
- **Barra di stato:** le dimensioni in pixel e lo zoom, per esempio `1920 × 1080` e `150%`. Per gli SVG le dimensioni vengono da `width` e `height`, o dal `viewBox`.
- Sulle immagini i tasti dello zoom non cambiano la dimensione del testo degli editor.

**Note sulla 7.2**
- **Scorrimento sincronizzato nei due versi,** come in VS Code: scorrendo l'editor, l'anteprima mostra in cima il blocco della prima riga visibile; scorrendo l'anteprima, l'editor va alla riga del blocco in cima.
  - L'anteprima è una lista con una riga per ogni blocco del Markdown (titolo, paragrafo, lista, tabella…). Keel trova dove comincia ogni blocco con lo stesso parser che usa gpui-component.
  - Per un quarto di secondo dopo uno scorrimento, il lato che è stato spostato non sposta l'altro: così i due non si rincorrono.
- **Link:**
  - una pagina web si apre nel browser;
  - `#titolo` porta al titolo, con le ancore di GitHub (`## Le tue prove` diventa `#le-tue-prove`);
  - un file del progetto, anche con spazi (`Il%20piano.md`), si apre in Keel nel gruppo dell'editor, non sopra l'anteprima.
- **Immagini:** quelle con percorso relativo si cercano nella cartella del file Markdown; quelle sul web si scaricano.
- **Liste di controllo:** un clic sulla casella mette o toglie la `x` nel testo, e si annulla con Cmd+Z nell'editor.
- **Un difetto della 7.1 corretto qui:** l'elemento della vista Markdown, se non glielo si dice, non scorre da solo, quindi i file lunghi restavano tagliati.

**Note sulla 7.1**
- **Aprire l'anteprima:**
  - Cmd+K V la apre in un gruppo a lato, come in VS Code;
  - Cmd+Maiusc+V la apre nello stesso gruppo, al posto dell'editor;
  - sui file Markdown c'è anche un pulsante in fondo alla fila delle schede;
  - i due comandi sono anche nella tavolozza.
- **Si aggiorna a ogni tasto,** anche col file non salvato.
- **La disegna gpui-component,** con i colori del tema e nello stile di GitHub; solo le righe visibili vengono disegnate, quindi anche i file lunghi scorrono leggeri:
  - titoli, tabelle, liste di controllo, testo barrato, link, citazioni;
  - blocchi di codice colorati con le grammatiche di Keel;
  - il testo si può selezionare e copiare.

  Il testo sta in una colonna di 880 punti al massimo, centrata, come su GitHub.
- **Provato su `docs/PIANO.md`:** si legge senza errori, tabelle comprese.

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

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 8.1 | Motore (crate `git`): storico con branch e tag, corsie e colori del grafico, dettagli e file di un commit, azioni, storico di un file, autore di ogni riga | fatta |
| 8.2 | Scheda «Git Graph»: grafico con linee colorate, descrizione, data, autore e hash; filtro per branch; ricerca; altri commit a richiesta | fatta |
| 8.3 | Dettaglio di un commit (file cambiati, differenze) e azioni dal menu: checkout, crea branch o tag, copia l'hash, cherry-pick, revert, merge, elimina branch, con conferma | fatta |
| 8.4 | Storico di un file e autore di ogni riga nell'editor, attivabile | fatta |
| 8.5 | Misure e resoconto | fatta |

**Resoconto della fase 8**
- **Il criterio era:** fai tutto quello che fai oggi con Git Graph. In Keel ci sono il grafico con i colori di Git Graph, branch, remoti e tag, la riga delle modifiche non committate, il filtro per branch, la ricerca, il dettaglio di un commit con i file e le differenze, e il menu col tasto destro con conferma prima di ogni cambiamento. **Resta la tua prova a mano.**
- **Rispetto a Git Graph mancano:** gli stash nel grafico, il reset del branch a un commit, il push di un tag, il tag annotato e il confronto fra due commit qualsiasi. Se te ne serve qualcuno, si aggiunge.
- **In più rispetto a Git Graph:** l'autore di ogni riga nell'editor e lo storico di un file.
- **Misure** (versione ottimizzata):

  | Repository | Lettura dello storico | Calcolo del grafico | Corsie |
  |---|---|---|---|
  | GestionaleHospitality, 134 commit | 12–45 ms | 0,04 ms | 1 |
  | Keel, 67 commit | 11–24 ms | 0,04 ms | 1 |
  | SailBrowser, 946 commit | 16 ms | 0,19 ms | 4 |

  - L'autore delle righe costa 54 ms su `sidebar.tsx` (713 righe) e 73 ms su un file di Keel di 3.123 righe, sempre fuori dalla finestra.
  - Il grafico disegna solo le righe in vista, quindi caricare 500 o 5.000 commit non cambia la fluidità.
- **Test:** 297 automatici, tutti verdi, più 14 da lanciare a mano.

**Le tue prove a mano della fase 8**
1. Apri GestionaleHospitality, apri la vista git (Ctrl+Maiusc+G) e premi il pulsante col grafico: si apre Git Graph.
2. Controlla linee, etichette dei branch e dei tag, date e autori. Con delle modifiche aperte, in cima c'è «Modifiche non committate».
3. Cerca una parola di un commit: Invio passa al risultato dopo.
4. Scegli un branch dal menu «Branch», poi torna a «Tutti i branch».
5. Clicca un commit: sotto compaiono i dettagli e i file. Clicca un file per le differenze.
6. Tasto destro su un commit: copia l'hash, crea un branch qui e poi eliminalo dal tasto destro sull'etichetta.
7. In un file di codice: «Git: Mostra o nascondi l'autore delle righe» dalla tavolozza. Scorri, scrivi una riga nuova, clicca un'etichetta.
8. Tasto destro su un file nell'Esplora risorse → «Git: Storico del file», e scegli un commit.

**Note sulla 8.4**
- **Autore di ogni riga** («Git: Mostra o nascondi l'autore delle righe», id `gitlens.toggleFileBlame` come in GitLens):
  - a sinistra dell'editor compare una colonna con data e autore, una sola volta dove comincia ogni commit, divisa da una linea sottile; segue l'editor quando scorre;
  - un clic su un'etichetta apre Git Graph sul commit;
  - nella barra di stato c'è autore, data e messaggio della riga del cursore;
  - conta anche le modifiche non salvate: una riga nuova è «Non ancora committato». Si aggiorna mezzo secondo dopo che smetti di scrivere, e dopo un commit o un checkout.

  GitLens scrive l'autore a fine riga, ma l'editor di gpui-component non sa disegnare testo dentro le righe: per questo è una colonna.
- **Storico di un file** («Git: Storico del file», o tasto destro su un file nell'Esplora risorse): l'elenco dei commit che l'hanno cambiato, anche prima di una rinomina, con hash, autore e data. Si filtra scrivendo, e quello scelto apre le sue modifiche al file.

**Note sulla 8.3**
- **Un clic su un commit** apre sotto il grafico il suo dettaglio:
  - a sinistra hash, genitori, autore con email e data, e il messaggio completo;
  - a destra i file cambiati, con la lettera colorata (A, M, D, R);
  - un clic su un file apre la scheda delle differenze del commit, affiancate o in linea, intitolata «file (hash)»;
  - un secondo clic sul commit chiude il dettaglio.
- **Tasto destro su un commit**, come in Git Graph:
  - Checkout (con HEAD staccato), Crea un branch qui, Aggiungi un tag, Cherry-pick, Revert, Copia l'hash, Copia il messaggio;
  - per ogni branch del commit: Checkout, Merge nel branch corrente, Elimina; per ogni tag: Elimina.
- **Prima di cambiare il repository Keel chiede conferma:** checkout di un commit, cherry-pick, revert, merge ed eliminazioni. I nuovi branch e tag chiedono il nome. Gli errori di git vanno nella barra di stato e nell'Output.
- **I tag nuovi sono semplici,** senza messaggio. Git Graph chiede anche se fare un tag annotato: se ti serve, lo aggiungo.

**Note sulla 8.2**
- **Si apre** dal pulsante col grafico in cima alla vista git, oppure da «Git Graph: Mostra il grafico» nella tavolozza. È una scheda come le altre.
- **Colonne:** Grafico, Descrizione, Data, Autore, Commit, come Git Graph.
  - Le corsie usano i 12 colori di Git Graph e si curvano dolcemente dove cambiano posto.
  - Il commit di HEAD ha il pallino vuoto.
  - I branch, i branch remoti e i tag sono etichette col colore della corsia; quello di HEAD è pieno.
- **In cima, se ci sono modifiche non committate,** c'è la riga «Modifiche non committate (N)» legata a HEAD; un clic apre la vista git.
- **Branch:** il menu in alto mostra tutti i branch oppure uno solo.
- **Ricerca:** cerca nel messaggio, nell'hash, nell'autore e nei nomi dei branch. Evidenzia le righe trovate, e Invio e Maiusc+Invio passano dall'una all'altra.
- **Commit:** se ne caricano 500 alla volta; in fondo «Carica altri 500 commit».
- **Il grafico si aggiorna da solo** quando cambia git: un commit, un checkout, anche dal terminale. In alto ci sono anche Fetch e Aggiorna.

**Note sulla 8.1**
- **Lo storico** si legge con `git log` in ordine di data, figli prima dei genitori, come Git Graph. Con ogni commit vengono anche i suoi branch, i branch remoti e i tag. Si può limitare a dei branch.
- **Le corsie del grafico** le calcola Keel:
  - ogni commit sta nella prima corsia che lo aspetta;
  - il primo genitore continua nella stessa corsia e con lo stesso colore;
  - gli altri genitori di un merge prendono una corsia nuova, con un colore nuovo;
  - le corsie finite lasciano il posto, e quelle a destra si spostano.
- **Di un commit:** messaggio completo, autore e data, e i file cambiati rispetto al primo genitore (un merge mostra quello che ha portato). Si legge anche la differenza di ogni file, rinomine comprese.
- **Comandi:** checkout di un commit, nuovo branch o tag su un commit (il tag annotato se ha un messaggio), elimina tag e branch, cherry-pick, revert, merge nel branch corrente. Per i merge, cherry-pick e revert seguono il primo genitore, come in Git Graph.
- **Storico di un file,** anche attraverso le rinomine, e **autore di ogni riga** (blame), anche con le modifiche non salvate dell'editor.

**Cosa si fa**
- **Grafico** di branch e commit con linee colorate, autore, data e messaggio; filtri per branch; ricerca nei commit.
- **Dettaglio di un commit:** file cambiati e differenze.
- **Azioni dal grafico:** checkout, crea branch o tag, copia l'hash, cherry-pick, revert (con conferma).
- **Storico di un file** e autore di ogni riga (blame, attivabile).

**Fatto quando**
- Fai tutto quello che fai oggi con Git Graph.

### Fase 9 — Estensioni

**Obiettivo.** Gestire le estensioni come in Cursor e far funzionare quelle che ti servono.

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 9.1 | Vista Estensioni: installate con icona e descrizione, attiva e disattiva (anche solo per il progetto), disinstalla, scheda dei dettagli con il README; installa da VSIX; host delle estensioni che si riavvia senza riaprire la finestra | fatta |
| 9.2 | Open VSX: ricerca dentro la vista, installazione, aggiornamenti | fatta |
| 9.3 | Temi colore e temi di icone dai VSIX (Dracula, Material Icon Theme), configurazioni di linguaggio | fatta |
| 9.4 | Isolamento (un'estensione bloccata o in crash non ferma Keel e si riavvia) e ponte `vscode` allargato per Peacock, gitignore e markdownlint | fatta |
| 9.5 | Importa da Cursor l'elenco delle estensioni | fatta |
| 9.6 | Misure e resoconto | fatta |

**Resoconto della fase 9**
- **Il criterio era:** VSAuto Ghost si installa dalla vista Estensioni come in Cursor e le estensioni della lista funzionano.
  - VSAuto Ghost si installa da VSIX, o con «Importa da Cursor» che la copia dalla cartella di Cursor.
  - Dracula, Material Icon Theme, Peacock, gitignore e markdownlint arrivano da Open VSX e funzionano: le prove automatiche le fanno girare con quello che usano davvero.
  - **Resta la tua prova a mano.**
- **Cosa c'è ora:**
  - vista Estensioni con ricerca su Open VSX, installazione, aggiornamenti, attiva e disattiva anche per progetto, disinstalla nel Cestino, dettagli col README;
  - temi colore e di icone dai VSIX, linguaggi delle estensioni;
  - host che si riavvia da solo se un'estensione si blocca, e che sopravvive agli errori delle estensioni;
  - import da Cursor.
- **Correzioni trovate strada facendo:**
  - le estensioni con `onLanguage:` non partivano mai (markdownlint);
  - un'estensione `*` che eseguiva un comando mentre si attivava restava bloccata (Peacock);
  - il lettore dei JSON con commenti diventava lentissimo con tante virgole.
- **Non ancora:**
  - i pannelli propri delle estensioni, le webview, i notebook, il debugger;
  - i language server avviati dalle estensioni;
  - i task delle estensioni e la loro formattazione.
  - Per questo Jupyter, Docker, Java, Dart e simili restano fuori, e «Importa da Cursor» lo dice.
- **Misure** (versione ottimizzata, progetto di prova con un README e due file):

  | | Keel | Language server | Primo disegno |
  |---|---|---|---|
  | Senza estensioni | 127 MB | 84 MB | 271–565 ms |
  | Con le sei estensioni (Dracula, Material Icon Theme, Peacock, gitignore, markdownlint, VSAuto Ghost) | 152 MB | 84 MB | 280–288 ms |

  - Le sei estensioni costano circa 25 MB, compreso V8; il primo disegno non cambia, perché le estensioni partono dopo.
  - Open VSX: una ricerca risponde in 0,14–0,18 s; scaricare Dracula (480 KB) richiede 0,7 s e markdownlint (350 KB) 0,3 s.
  - Tradurre il tema Dracula richiede 3 ms; leggere Material Icon Theme (1.256 icone) 25 ms.
  - «Importa da Cursor» controlla le tue 47 estensioni in circa 5 secondi.
  - Un host bloccato viene riavviato al più 15 secondi dopo: 5 tra un controllo e l'altro, più 10 di attesa.
- **Test:** 322 automatici, tutti verdi, più 19 da lanciare a mano: tra questi le prove sulle estensioni vere e quella sul vero Open VSX.

**Le tue prove a mano della fase 9**
1. Vista Estensioni (Cmd+Maiusc+X): cerca «dracula» e installa «Dracula Theme». Poi «Preferenze: Tema colori» e scegli «Dracula Theme».
2. Installa «Material Icon Theme», poi «Preferenze: Tema icona file» e sceglilo. Controlla le icone nell'Esplora risorse; torna a «Icone di Keel» se non ti piace.
3. Menu «…» della vista → «Importa da Cursor…». Guarda i tre gruppi e premi «Installa le disponibili», oppure installa solo quelle che vuoi.
4. In un progetto: «Peacock: Change to a Favorite Color». La barra del titolo e quella di stato si colorano, e il colore finisce in `.vscode/settings.json` del progetto (lo vede anche Cursor).
5. Apri un file Markdown con spazi in fondo a una riga: markdownlint lo segnala nel pannello Problemi. Mettiti sulla riga e premi Cmd+.: «Fix this violation of MD009…» lo corregge.
6. In un progetto senza `.gitignore`: «Add gitignore», scegli «Node» e controlla il file creato.
7. Sull'ingranaggio di un'estensione: «Disattiva per questo progetto», poi «Attiva». Il suo tema o le sue icone spariscono e tornano senza riaprire la finestra.
8. Disinstalla un'estensione: la conferma e poi la cartella nel Cestino.

**Note sulla 9.5**
- **«Importa da Cursor…»** sta nel menu «…» della vista Estensioni e nella tavolozza («Estensioni: Importa da Cursor»).
- **Keel legge l'elenco** di `~/.cursor/extensions/extensions.json` senza toccare nulla di Cursor.
- **Divide le estensioni in tre gruppi:**
  - da installare: quelle su Open VSX, e quelle fatte da te come VSAuto Ghost, che vengono copiate dalla cartella di Cursor;
  - già in Keel: quelle già installate, e quelle che Keel fa da solo (Prettier, Git Graph, Claude Code, PDF, anteprima Markdown, YAML, TypeScript);
  - non funzionano in Keel, col motivo: fatta solo per Cursor, serve il debugger, servono i notebook, pannelli propri, avvia un language server, oppure serve un'altra estensione che non funziona, oppure non è su Open VSX.
- **«Senza: pagine web interne» o «editor propri»:** l'estensione si installa e funziona in parte, come Peacock.
- **Ogni estensione ha il suo pulsante «Installa»;** in cima, «Installa le disponibili (N)» le installa tutte.
- **Con le tue 47 estensioni di Cursor** il controllo dura circa 5 secondi, perché chiede a Open VSX 8 estensioni alla volta.
  - Da installare: 18 (17 da Open VSX più VSAuto Ghost), tra cui Dracula, Material Icon Theme, Peacock, gitignore e markdownlint.
  - Già in Keel: 7.
  - Non funzionano: 22, come Jupyter, Docker, Java, Dart e Flutter, Remote SSH.
- **Il riconoscimento è a buon senso:** guarda il `package.json` e cerca nel codice principale le webview e i language client. Qualche estensione «da installare» può comunque non servire a nulla in Keel, per esempio le scorciatoie di Jupyter.

**Note sulla 9.4**
- **Estensione bloccata:** ogni 5 secondi la finestra chiede all'host delle estensioni se risponde.
  - Se per 10 secondi non risponde, o se si è fermato, Keel lo riavvia da solo.
  - L'Output dice cosa stava facendo, per esempio `commands/execute stuck.loop`.
  - Dopo 3 riavvii in 10 minuti le estensioni restano spente finché non le riavvii dal menu «…» della vista Estensioni.
- **Errore di un'estensione:** un errore non gestito dentro una promessa, un timer o un `queueMicrotask` va nell'Output e non ferma più l'host con le altre estensioni.
- **Peacock:**
  - si attiva anche con un progetto aperto (prima restava in attesa di sé stesso);
  - salva i colori nel `.vscode/settings.json` del progetto, come in VS Code e Cursor, che quindi li vede anche lui;
  - Keel colora con `workbench.colorCustomizations` la barra del titolo e la barra di stato;
  - le varianti per tema (`"[Dracula Theme]": {...}`) valgono col tema giusto;
  - un salvataggio con lo stesso valore non annuncia più un cambio, come in VS Code: prima Peacock salvava all'infinito.
- **gitignore:** «Add gitignore» scarica l'elenco dei modelli da GitHub, te lo fa scegliere e scrive `.gitignore`. Le richieste web passano da curl e possono andare solo a indirizzi `http` e `https`.
- **markdownlint:**
  - si attiva quando apri un file Markdown: ora Keel attiva le estensioni con `onLanguage:`, prima no;
  - i suoi avvisi compaiono nel pannello Problemi, sottolineati nel testo e nelle icone dell'Esplora risorse, col nome «markdownlint»;
  - Cmd+. offre le sue correzioni insieme a quelle dei language server.
- **Moduli di Node per le estensioni:** `fs`, `os`, `events`, `util`, `url`, `stream`, `http`, `https` e pochi altri, ridotti a quello che usano queste estensioni.
  - Non possono avviare programmi (`child_process`) né aprire connessioni dirette (`net`, `tls`): ricevono un errore chiaro.
  - `process.env` mostra solo HOME, USER, LANG, TMPDIR, SHELL e PATH, non le chiavi segrete della shell.
- **Non ancora:** i task delle estensioni (markdownlint «Lint workspace») e la formattazione offerta dalle estensioni. Le estensioni le registrano senza errori, ma Keel per ora non le usa.

**Note sulla 9.3**
- **Temi colore delle estensioni**, come Dracula:
  - compaiono in «Preferenze: Tema colori» dopo i tre di Keel, con il nome che hanno in VS Code;
  - Keel traduce i colori di VS Code in quelli dei suoi pannelli, liste, schede e pulsanti, e i colori del codice nei nomi che usa il suo editor;
  - i colori che un tema non dà li ricava dai suoi due colori principali, sfondo e testo, così nulla prende i colori di un altro tema;
  - funzionano anche i temi con commenti e quelli che ne includono un altro, come i temi di VS Code.
- **Temi di icone**, come Material Icon Theme:
  - si scelgono con «Preferenze: Tema icona file», che salva `workbench.iconTheme` come VS Code;
  - la prima voce, «Icone di Keel», torna alle icone di sempre;
  - l'icona di un file si cerca per nome intero, poi per estensione dalla più lunga (`app.test.ts` prova `test.ts` e poi `ts`), poi per linguaggio;
  - le cartelle hanno l'icona per nome, aperte o chiuse;
  - con un tema chiaro valgono le varianti chiare del tema di icone;
  - per ora le icone cambiano nell'Esplora risorse; le schede restano senza icona come prima.
- **Linguaggi delle estensioni:**
  - un file che Keel non conosce prende il linguaggio dall'estensione che lo dichiara, per nome o per estensione;
  - così le estensioni e i language server di quel linguaggio lo ricevono;
  - «Commenta riga» e «Commenta blocco» usano i commenti della sua `language-configuration.json`.
  - Le grammatiche TextMate non si usano: i colori del codice vengono dalle grammatiche tree-sitter di Keel.
- **Spegnendo un'estensione** i suoi temi spariscono dal selettore e le sue icone e i suoi linguaggi non valgono più.
  - Un tema già registrato resta in memoria finché Keel non si chiude: una versione nuova dello stesso tema si vede dopo aver riaperto Keel.
- **Correzione trovata strada facendo:** il lettore dei file JSON con commenti, usato anche per `settings.json`, per ogni virgola rileggeva tutto il resto del testo, quindi il tempo cresceva in modo esponenziale. Ora legge una volta sola: 20.000 voci in meno di un secondo.

**Note sulla 9.2**
- **In cima alla vista Estensioni** c'è la casella «Cerca estensioni in Open VSX».
  - Mentre scrivi, dopo una breve pausa, compaiono i risultati di Open VSX al posto delle installate. Ogni risultato ha icona, nome, versione, descrizione, editore e numero di download.
  - Svuotando la casella tornano le installate.
- **Il pulsante di ogni risultato** dice «Installa», «Aggiorna» se ne hai una versione più vecchia, oppure «Installata».
- **L'installazione:**
  - Keel scarica la versione per il Mac con chip Apple quando esiste, altrimenti quella per tutti;
  - controlla il pacchetto con lo SHA-256 pubblicato da Open VSX;
  - lo installa come un VSIX e riavvia le estensioni;
  - il file scaricato si cancella subito.
- **Gli aggiornamenti** si controllano una volta all'apertura della finestra, come in VS Code. Si spengono con `"extensions.autoCheckUpdates": false`.
  - Le estensioni con una versione nuova hanno il pulsante «Aggiorna» nell'elenco.
  - Il menu «…» ha «Controlla aggiornamenti estensioni» e, quando ce ne sono, «Aggiorna tutte le estensioni». Si trovano anche nella tavolozza.
  - Le estensioni che non stanno su Open VSX, come VSAuto Ghost locale, si saltano.

**Note sulla 9.1**
- **La vista Estensioni** elenca tutte le estensioni installate, in ordine di nome, con icona, versione, descrizione ed editore. Quelle spente hanno la scritta «Disattivata».
- **L'ingranaggio di ogni estensione** offre:
  - Disattiva;
  - Disattiva per questo progetto;
  - Attiva;
  - Disinstalla.
- **Disinstalla** chiede prima conferma e manda la cartella nel Cestino, da cui si può recuperare.
- **Le scelte** stanno in `extension-state.json` nella cartella dati di Keel. Valgono per tutte le finestre aperte dopo.
- **Un clic su un'estensione** apre la sua scheda con il README e un riassunto di cosa porta: comandi, scorciatoie, temi, temi di icone, snippet, linguaggi.
- **Il menu «…» in cima alla vista** ha «Installa da VSIX…» e «Riavvia le estensioni». Si trovano anche nella tavolozza dei comandi.
  - Installando una versione nuova, quella vecchia va nel Cestino.
- **L'host delle estensioni si riavvia da solo** dopo ogni installazione, attivazione o disattivazione, senza riaprire la finestra.
  - I file aperti vengono ripresentati alle estensioni.
  - Un'estensione bloccata in un ciclo infinito viene fermata entro 2 secondi, quindi non trattiene la finestra.

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

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 10.1 | Collegamento IDE: Keel si annuncia in `~/.claude/ide/` come VS Code e Cursor; Claude Code nei terminali di Keel vede cartelle, schede aperte, selezione e Problemi, apre e salva file; Cmd+Alt+K gli passa il file e le righe | fatta |
| 10.2 | Modifiche proposte da Claude Code nel terminale: si aprono in Keel come differenze da accettare o rifiutare | fatta |
| 10.3 | Crate `ai` per ACP: avvio dell'adattatore `claude-agent-acp` (installato da Keel con versione bloccata), sessioni, permessi, file letti e scritti attraverso Keel | fatta |
| 10.4 | Pannello agente a destra, accanto a Claude Code nel terminale: conversazione, ragionamento, strumenti, piano, permessi, interrompi, riferimenti a file e selezione | fatta |
| 10.5 | Modifiche dell'agente file per file da accettare o rifiutare; cronologia delle conversazioni | fatta |
| 10.6 | Misure e resoconto | fatta |

**Resoconto della fase 10**
- **Il criterio era:** lavori con Claude Code dal terminale e dal pannello, e ogni modifica la vedi e la controlli in Keel.
  - **Dal terminale:** Claude Code si collega alla finestra come a VS Code e Cursor. Vede file, selezione e Problemi, e le modifiche che propone si accettano o si rifiutano in una scheda di Keel. Provato col vero Claude Code 2.1.303.
  - **Dal pannello:** l'agente ACP con conversazione, strumenti, piano, permessi, modalità e cronologia. Le sue modifiche sono elencate file per file, da accettare o rifiutare.
  - **Resta la tua prova a mano:** i test automatici usano un agente finto e la prova con quello vero non ha mandato messaggi, per non consumare il tuo uso di Claude.
- **Correzioni trovate strada facendo:**
  - Claude Code vuole il sottoprotocollo WebSocket `mcp`;
  - la prima versione del collegamento rispondeva in 26 ms per un giro di controllo: ora risponde in 0,09 ms e a riposo non lavora;
  - chiudendo Keel l'annuncio in `~/.claude/ide` sparisce e l'adattatore si ferma.
- **Misure** (versione ottimizzata):
  - Keel con il collegamento per Claude Code: 120–137 MB (127 MB nella fase 9); primo disegno 330–475 ms;
  - una chiamata di Claude Code a Keel, andata e ritorno: 0,09 ms di mediana, 0,17 ms la più lenta su 200;
  - l'adattatore ACP: installazione 2 minuti e 18 secondi una volta sola; avvio 0,2–0,46 s; sessione aperta in 0,9–3,7 s;
  - a riposo l'adattatore pesa 461 MB (node 126 MB più il programma `claude` 335 MB), come Claude Code nel terminale.
- **Non ancora:** i comandi `/` di Claude nel campo del pannello, le immagini incollate nel pannello, la riapertura automatica dell'ultima conversazione.
- **Test:** 334 automatici, tutti verdi (cinque volte di fila), più 22 da lanciare a mano.

**Le tue prove a mano della fase 10**
1. Apri un progetto in Keel e premi Cmd+Esc: Claude Code parte a destra. Scrivi `/ide`: deve dire che è collegato a «Keel».
2. Seleziona qualche riga in un file e chiedi a Claude nel terminale «cosa ho selezionato?». Poi prova Cmd+Alt+K su una selezione.
3. Chiedi a Claude nel terminale una piccola modifica a un file: deve aprirsi la scheda con le differenze. Prova «Rifiuta», poi rifallo e «Accetta».
4. Passa a «Agente» in cima al riquadro di destra. La prima volta aspetta l'installazione, circa 2 minuti, poi scrivi un messaggio.
5. Chiedi all'agente una modifica: guarda la richiesta di permesso con la differenza, consentila, poi usa «Modifiche di Claude» per aprire le differenze e rifiutarla.
6. Prova «@» con una selezione, il menu della modalità (per esempio «Piano») e «Interrompi» durante una risposta lunga.
7. Apri l'orologio in alto a destra e riprendi una conversazione passata, anche una iniziata nel terminale.

**Note sulla 10.5**
- **Prima che Claude cambi un file**, Keel ne tiene una copia: appena vede arrivare la modifica, anche quando la modalità non chiede permessi.
- **Sopra il campo del pannello** compare «Modifiche di Claude: N file». Ogni file ha le righe aggiunte e tolte (+3 −1), con «nuovo» se l'ha creato Claude.
  - Un clic sul file apre una scheda con le differenze tra la copia e il file adesso.
  - ✓ accetta: la modifica resta e il file esce dall'elenco.
  - ↶ rifiuta: il file torna com'era, anche nell'editor; un file creato da Claude va nel Cestino.
  - «Accetta tutte» e «Rifiuta tutte» fanno lo stesso per tutti; la scheda delle differenze ha gli stessi due pulsanti.
- **Un file che ritorna come prima** (per esempio perché Claude stesso annulla) esce dall'elenco da solo.
- **Le copie restano** anche passando a un'altra conversazione, finché non accetti o rifiuti.
- **La cronologia delle conversazioni** è arrivata con la 10.4: è quella di Claude Code, e si riprende dall'orologio.

**Note sulla 10.4**
- **In cima al riquadro di destra** ci sono due interruttori: «Terminale» (Claude Code nel terminale, come prima) e «Agente», il pannello nuovo. Si apre anche con Cmd+Maiusc+Esc o «Claude Code: Apri l'agente».
- **La prima volta** Keel installa l'adattatore, in circa 2 minuti, e lo dice nel pannello. Poi Claude parte in pochi secondi nella cartella del progetto.
- **Nella conversazione:**
  - i tuoi messaggi a destra;
  - le risposte di Claude in Markdown, che arrivano mentre scrive;
  - il ragionamento chiuso in una riga «Ragionamento» che si apre con un clic;
  - gli strumenti con icona, titolo e stato (… ✓ ✗) e le differenze dei file in rosso e verde: un clic apre il file alla riga giusta.
- **Il piano** di Claude sta sopra il campo, con le voci fatte, in corso e da fare.
- **Le richieste di permesso** compaiono sopra il campo, con la differenza del file e i pulsanti di Claude («Allow», «Always allow», «Reject»…).
- **Il campo:**
  - Invio invia, Maiusc+Invio va a capo;
  - «@» aggiunge il file aperto, o le righe selezionate col loro testo, e si vedono come etichette;
  - il menu della modalità cambia come Claude chiede i permessi (Manuale, Accetta modifiche, Piano, Auto…);
  - durante un turno il pulsante diventa «Interrompi».
- **In alto a destra:** «Nuova conversazione» (+) e le conversazioni passate del progetto (orologio). Sono quelle di Claude Code stesso, anche quelle iniziate nel terminale, e si riprendono da dove erano.
- **Cosa resta fuori per ora:**
  - i comandi `/` di Claude nel campo;
  - le immagini incollate;
  - la conversazione non si riapre da sola quando riapri la finestra, ma si ritrova nell'orologio.

**Note sulla 10.3**
- **Il crate `ai`** parla ACP con l'agente: un messaggio JSON per riga sul suo input e output.
  - Keel apre le sessioni, manda i messaggi, li interrompe e cambia modalità.
  - Risponde alle richieste dell'agente: leggere e scrivere file attraverso Keel, chiedere il permesso.
- **La conversazione** si costruisce dagli aggiornamenti dell'agente:
  - messaggi che arrivano a pezzi e ragionamento;
  - strumenti con il loro stato e le differenze dei file;
  - piano, comandi `/`, modalità e titolo.
- **L'adattatore** è `@agentclientprotocol/claude-agent-acp` 0.107.3, bloccato come i language server. Keel lo installa con npm nella sua cartella `servers`.
  - Ci mette poco più di 2 minuti, una volta sola: 115 pacchetti, tra cui l'SDK di Claude Code.
  - Usa il tuo login di Claude Code: nessuna chiave in Keel.
- **Provato con l'adattatore vero, senza mandare messaggi:**
  - risponde in 0,46 s e apre una sessione in 3,7 s;
  - elenca le tue conversazioni di Claude Code con il loro titolo, quindi la cronologia è quella di Claude Code stesso;
  - offre le modalità Manuale, Accetta modifiche, Piano, Auto e Bypass, e i tuoi comandi `/` e skill.
- **Come modifica i file:** questa versione dell'adattatore li scrive da sé, con gli strumenti di Claude Code, e prima chiede il permesso mostrando la differenza. La revisione file per file della 10.5 parte da qui.
- **Per i test** c'è un agente finto che risponde come l'adattatore senza modello: conversa, chiede il permesso, modifica un file, si interrompe, ricorda le sessioni.

**Note sulla 10.2**
- **Quando Claude Code nel terminale vuole cambiare un file**, Keel apre una scheda con le differenze tra il file com'è sul disco e la proposta.
  - Il titolo della scheda è quello che dà Claude Code; si vede affiancata o in una colonna, come le altre differenze.
  - Un file nuovo appare tutto in verde.
- **«Accetta» e «Rifiuta»** stanno in cima alla scheda e nella tavolozza: «Claude Code: Accetta la modifica proposta» e «Claude Code: Rifiuta la modifica proposta», gli stessi id di VS Code.
- **Accettando,** Claude Code riceve il testo e scrive lui il file, come con VS Code; Keel lo ricarica da solo.
- **Chiudere la scheda** vale come «Rifiuta». Una proposta con lo stesso nome prende il posto della precedente.
- **Le richieste di Claude Code di chiudere una scheda o tutte** sono servite; quelle ancora in attesa diventano un rifiuto.

**Note sulla 10.1**
- **Ogni finestra di Keel si annuncia a Claude Code** come fanno VS Code e Cursor:
  - apre un collegamento solo su questo Mac (127.0.0.1) con un codice segreto;
  - lo scrive in `~/.claude/ide/<porta>.lock`, leggibile solo da te, insieme alla cartella del progetto;
  - quando la finestra si chiude, il file sparisce.
- **I terminali di Keel** ricevono `CLAUDE_CODE_SSE_PORT`, quindi `claude` avviato lì (anche con Cmd+Esc) si collega da solo alla sua finestra. Da un terminale esterno basta `/ide` e scegliere «Keel».
- **Cosa vede Claude Code:**
  - le cartelle e le schede aperte;
  - il file e la selezione dove sei: Keel glieli dice appena il cursore si muove;
  - i Problemi, e quando cambiano.
- **Cosa può fare Claude Code:** aprire un file selezionando un pezzo di testo, sapere se un file ha modifiche non salvate, salvarlo.
- **Cmd+Alt+K** («Claude Code: Inserisci il riferimento al file», come in VS Code) mette nel messaggio di Claude Code il file e le righe selezionate.
- **Provato col vero Claude Code 2.1.303:**
  - si collega, si presenta e chiede l'elenco degli strumenti;
  - vuole il sottoprotocollo WebSocket `mcp`: senza conferma restava zitto.
  - Per rifare la prova: `KEEL_CLAUDE_PROJECT=<cartella fidata> KEEL_PTY_RUN=<pty_run.py> cargo test -p ai -- --ignored real_claude --nocapture`. Con `KEEL_IDE_TRACE=1` Keel stampa i messaggi ricevuti.
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

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 11.1 | Colori e riconoscimento dei file: Python, Java, C, C++, C#, Bash e zsh, Dockerfile, SQL, XML, Makefile e `.properties` | fatta |
| 11.2 | Motore dei server: archivi `tar.gz` e `zip` con controllo SHA-256; programmi necessari (Java, .NET, strumenti di Xcode) e guida a installarli quando mancano | fatta |
| 11.3 | Python: `basedpyright` e `ruff`, con l'ambiente virtuale del progetto (`.venv`, uv, Poetry) | fatta |
| 11.4 | Java con `jdtls` (Maven e Gradle) e Spring Boot (`application.properties` e `.yml`) | fatta |
| 11.5 | C e C++ con `clangd` (`compile_commands.json`, clang-format); C# con .NET | fatta |
| 11.6 | Linguaggi di base: Bash e zsh (con ShellCheck e shfmt), Dockerfile, SQL, XML, Makefile | fatta |
| 11.7 | Prove su un progetto per ogni linguaggio, misure e resoconto | fatta |

**Resoconto della fase 11**
- **Il criterio era:** per ogni linguaggio un progetto di prova si comporta come in VS Code.
  - **Come in VS Code:** i server sono quelli delle estensioni di VS Code più usate. Per Java, Spring Boot e XML sono proprio i loro pacchetti. Keel fa anche i passaggi che in VS Code fanno le estensioni:
    - i comandi di jdtls;
    - le domande di Spring Boot passate a jdtls;
    - le classi delle librerie;
    - l'ambiente virtuale di Python.
  - **Provato con i server veri:**
    - Python su Hondana;
    - Java e Spring Boot su un progetto Maven creato con start.spring.io;
    - C++ con CMake;
    - C# con `dotnet new console`;
    - un file per Bash, Dockerfile, XML e SQL.

    Errori, completamenti, info, definizioni, riferimenti e formattazione funzionano (tempi nelle note di ogni sottofase).
  - **Resta la tua prova a mano.**
- **Misure** con Keel ottimizzato, aprendo un file dal comando `keel`, dopo 25–60 secondi:

  | File | Keel | Server | Primo fotogramma |
  |---|---|---|---|
  | Hondana, `hondana/api.py` | 123 MB | basedpyright 192 MB, Ruff 15 MB | 530 ms |
  | Spring Boot, `ProvaApplication.java` | 114–127 MB | jdtls 112–333 MB, Spring Boot 88–357 MB | 300–375 ms |
  | C++, `main.cpp` | 133 MB | clangd 318 MB | 316 ms |
  | C#, `Program.cs` | 111 MB | csharp-ls 125 MB | 328 ms |
  | Bash, `deploy.sh` | 106–141 MB | Bash IDE fino a 630 MB nei primi secondi, poi 192 MB | 290–334 ms |

  - **Keel resta sui 106–141 MB**, come nelle fasi 9 e 10. A riposo usa meno dello 0,5% di CPU.
  - **I server pesano più di Keel**, come nella fase 6: sono gli stessi programmi di VS Code. jdtls e Spring Boot crescono mentre importano il progetto, poi Java restituisce memoria.
  - Chiudendo Keel tutti i server si fermano.
- **Sistemato durante le misure:**
  - **Un file aperto da solo**, fuori da un progetto con `.git`, `.vscode` o `.keel`, faceva partire i server nella cartella del file: jdtls non vedeva il `pom.xml` e Spring Boot non partiva. Ora i server partono dal primo file di build risalendo (`pom.xml`, `build.gradle`, `CMakeLists.txt`, `*.csproj`, `pyproject.toml`…), come fanno i client dei linguaggi degli altri editor.
  - **L'ambiente Python in una sottocartella** (Hondana ha `.venv` in `backend/`, mentre il progetto è `Hondana`): ora Keel lo cerca anche nelle cartelle di primo livello, come l'estensione Python di VS Code.
  - **`KEEL_LSP_TRACE=1`** scrive nel terminale il nome di ogni messaggio dei server: serve per capire cosa succede senza aprire l'Output.
- **Rimandati:**
  - i completamenti di SQL;
  - Gradle con un progetto vero;
  - le funzioni dentro le classi di libreria (sono in sola lettura, senza info né salti);
  - le impostazioni `java.jdt.ls.vmargs` per cambiare la memoria di jdtls.
- **Test:** 355 automatici, tutti verdi per quattro volte di fila. Più 24 da lanciare a mano: server veri e misure, con la nuova prova di jdtls e Spring Boot insieme (`cargo test -p lsp --test real_spring -- --ignored`).

**Le tue prove a mano della fase 11**

La prima volta che apri un file di un linguaggio, Keel scarica il suo server: nella barra di stato compare «Installo il server…». jdtls pesa 133 MB, Spring Boot 83 MB, gli altri pochi MB.
1. **Python:** apri Keel su Hondana e poi `backend/hondana/api.py`. Passa sopra a un `import fastapi`: l'info arriva dal `.venv` del progetto. Scrivi `x: int = "a"`: errore rosso. Ruff segnala gli import non usati; Maiusc+Alt+F formatta.
2. **Java:** apri un tuo progetto Maven o Gradle (o creane uno con start.spring.io) e un file `.java`. La prima volta l'import del progetto richiede qualche decina di secondi (la barra di stato lo mostra), poi errori, completamenti e Cmd+. per gli import mancanti. F12 su una classe di Spring apre il suo sorgente in sola lettura.
3. **Spring Boot:** nello stesso progetto apri `src/main/resources/application.properties` e scrivi `server.po`: compare `server.port`. Una proprietà sbagliata viene segnalata.
4. **C e C++:** apri un progetto con `CMakeLists.txt`; se `build/compile_commands.json` non c'è, lancia `cmake -S . -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON`. Errori, completamenti e F12 anche nelle intestazioni di sistema.
5. **C#:** apri un file `.cs`. Senza .NET compare l'avviso «Per il server C# serve .NET 10»: prova «Installa nel terminale» (chiede la tua password), poi «Linguaggi: Riavvia i language server».
6. **Linguaggi di base:**
   - uno script `.sh` con una variabile senza virgolette: avviso di ShellCheck;
   - un `Dockerfile` con `RUNN`: errore;
   - un `pom.xml` con un tag chiuso male: errore;
   - un file `.sql`: avvisi di stile e formattazione.
7. **Makefile:** apri un `Makefile` e vai a capo dentro un target: il rientro è un tab (in basso a destra c'è «Tab»), non spazi.
8. **Colori:** apri un file di ogni linguaggio. Python, Java, C, C++, C#, SQL, XML, Dockerfile, Makefile e `.properties` hanno i loro colori.

**Decisioni prese all'inizio**
- **I server sono quelli delle estensioni di VS Code** più usate, così il comportamento è lo stesso: Pylance si basa su Pyright (qui `basedpyright`, la sua versione libera); Java usa `jdtls` come l'estensione di Red Hat; XML usa `lemminx`, sempre di Red Hat.
- **Keel installa da solo** i server che si scaricano (npm, archivi dalle pagine di rilascio). **Non installa da solo** i programmi di sistema, cioè Java, .NET e gli strumenti di Xcode: se mancano ti dice cosa manca e ti offre il comando da lanciare nel terminale.

**Note sulla 11.6**
- **Bash e zsh:** il server di Bash IDE, l'estensione più usata in VS Code. Dà errori, completamenti, info e struttura del file.
  - Usa **ShellCheck** per gli avvisi e **shfmt** per formattare. In VS Code li installi tu; Keel li scarica da solo, con l'impronta SHA-256, e li mette in testa al `PATH` del server. Se il download fallisce, il server parte lo stesso e l'Output dice cosa manca.
  - Su zsh ShellCheck non lavora (non lo supporta), come in VS Code.
- **Dockerfile:** il server dell'estensione Docker di VS Code. Dà istruzioni sconosciute, porte sbagliate, completamenti e formattazione. Per `compose.yaml` resta il server YAML della fase 6, con lo schema di Compose.
- **XML:** `lemminx`, il server dell'estensione XML di Red Hat, nella versione nativa che non vuole Java (16 MB). Ha le stesse impostazioni predefinite di VS Code:
  - un file senza schema riceve solo un suggerimento;
  - gli schemi nominati dai file (come quello di Maven in `pom.xml`) si scaricano e controllano il file.
- **SQL:** `sqruff`, che dà gli avvisi di stile e formatta. VS Code da solo non ha niente per SQL, oltre ai colori.
  - Ho scartato `sqls`: per Mac esiste solo per Intel e senza un database collegato dà poco.
  - **Mancano i completamenti** delle parole di SQL: sqruff non li fa.
- **Makefile:** solo colori, come in VS Code, che non ha un server. Però i Makefile ora si scrivono con i tab veri, come la regola predefinita di VS Code: i comandi dei target li vogliono.
  - **Tab e spazi per linguaggio.** `editor.tabSize` e `editor.insertSpaces` valgono anche dentro `"[python]": { ... }` e simili, come in VS Code; prima valevano per tutti i file.
- **Prove con i server veri** su file di prova:

  | Server | Installazione | Primo errore | Formattazione |
  |---|---|---|---|
  | Bash (con ShellCheck e shfmt) | 3,3 s | «Double quote to prevent globbing» | 186 ms |
  | Dockerfile | 1,7 s | «Unknown instruction: RUNN» | meno di 1 ms |
  | XML | 1,1 s | tag chiuso male | meno di 1 ms |
  | SQL | 0,8 s | spazi mancanti | 2 ms |

**Note sulla 11.5**
- **C, C++ e Objective-C** usano `clangd` degli strumenti di Xcode (Apple clangd 21), che conosce già le intestazioni dell'SDK di macOS. Se il `PATH` ha prima l'LLVM di Homebrew, vale quello. Keel non lo installa: se mancano gli strumenti di Xcode propone `xcode-select --install`.
  - Le opzioni di compilazione vengono da `compile_commands.json`, nella cartella del progetto o in `build/`. CMake lo scrive con `CMAKE_EXPORT_COMPILE_COMMANDS`; senza, clangd usa opzioni generiche, come nell'estensione di VS Code.
  - La formattazione usa il `.clang-format` del progetto.
- **C#** usa `csharp-ls` 0.28, un pacchetto NuGet (22 MB, con impronta SHA-256) che Keel avvia con `dotnet`. Carica da solo la soluzione (`.sln`, `.slnx`) o i `.csproj` della cartella.
  - Serve **.NET 10**, che sul Mac non c'è. Keel lo dice e propone `brew install --cask dotnet-sdk`, che chiede la tua password. Per le prove ho scaricato l'SDK ufficiale di .NET 10 nella cartella di prova (impronta SHA-512 di Microsoft verificata), senza installarlo nel sistema.
  - VS Code con C# Dev Kit usa il server Roslyn di Microsoft. Quello vuole messaggi suoi per aprire la soluzione e scarica .NET da sé. csharp-ls usa lo stesso Roslyn con il protocollo normale.
  - I messaggi di errore di C# sono in italiano, perché .NET segue la lingua del Mac.
- **Errori chiesti di nuovo a fine lavoro.** csharp-ls chiede all'editor di domandargli gli errori, ma lo dice prima di aver caricato il progetto, e dopo non avvisa. Keel ora richiede gli errori dei file aperti ogni volta che un server finisce un lavoro (`$/progress` «end»), per esempio il caricamento del progetto.
- **Prove con i server veri** su un progetto C++ con CMake e su uno C# creato con `dotnet new console`:

  | Prova | Tempo |
  |---|---|
  | clangd, avvio | 29 ms |
  | clangd, primi errori (con le intestazioni di sistema) | 0,64 s |
  | clangd, completamento / info / definizione / riferimenti | 15 ms / 3 ms / meno di 1 ms / meno di 1 ms |
  | csharp-ls, installazione | 0,9 s |
  | csharp-ls, avvio | 0,3 s |
  | csharp-ls, primi errori (progetto caricato) | 2 s |
  | csharp-ls, completamento: primo / dopo | 175 ms / 12 ms |
  | csharp-ls, info / riferimenti / formattazione | 0,7 s / 94 ms / 112 ms |

**Note sulla 11.4**
- **Java con il pacchetto dell'estensione di VS Code.** Keel scarica l'estensione Java di Red Hat per Mac Apple (133 MB, con impronta SHA-256). Dentro ci sono jdtls 1.58, il Java 25 su cui gira e Lombok: non serve installare Java.
  - Ho provato prima jdtls 1.63 dal sito di Eclipse: vuole Java 25 anche se il suo script dice 21, e sul Mac c'è Java 21. VS Code evita il problema allo stesso modo, portandosi il suo Java.
  - Keel lancia jdtls con le stesse opzioni di memoria di VS Code (parte da 100 MB, al massimo 2 GB). Lo lancia direttamente, senza lo script Python.
  - L'indice di ogni progetto sta nella cartella del server dentro i dati di Keel (`workspaces/<progetto>-<impronta>`), non nella cache di sistema.
  - **Lombok:** se `pom.xml` o `build.gradle` lo nominano, jdtls parte con il suo agente, come in VS Code. Senza, i progetti che lo usano sarebbero pieni di falsi errori.
  - **Maven e Gradle** si importano da soli. Dopo una modifica a `pom.xml` o `build.gradle` Keel aggiorna il progetto senza chiedere: VS Code chiede, Keel non ha dove farlo.
- **Le classi delle librerie** (Vai alla definizione su `SpringApplication`) si aprono in una scheda in sola lettura, con il testo che dà jdtls: il sorgente se Maven lo ha scaricato, altrimenti la classe ricostruita.
- **Le azioni rapide di Java** portano la modifica in un comando (`java.apply.workspaceEdit`): Keel lo esegue da sé, come l'estensione di VS Code.
- **Spring Boot** usa il server dell'estensione di VS Code (VMware, 83 MB), solo nei progetti il cui `pom.xml` o `build.gradle` nomina `spring-boot`.
  - `application*.properties` e `application*.yml` (anche `bootstrap` e i profili come `application-dev.yml`) diventano file di Spring Boot, con completamenti, errori e info sulle proprietà. Fuori dai progetti Spring restano normali file YAML e properties.
  - Il server gira sul Java di jdtls.
  - **Non lavora da solo:** chiede a jdtls il classpath e i tipi passando dall'editor. Keel fa lo stesso passaggio di VS Code:
    - jdtls parte con le 5 parti di Spring Boot (`jars/`). Per questo nei progetti Spring Keel installa prima Spring Boot, poi jdtls.
    - Le domande di Spring Boot (`sts/addClasspathListener`, `sts/javaType`…) vanno a jdtls come comandi `sts.java.…`.
    - Le risposte di jdtls al cambiare del classpath (`sts4.classpath.…`) tornano a Spring Boot.
    - Due installazioni dello stesso server non si sovrappongono più: la seconda aspetta la prima.
- **Prove con i server veri** su un progetto Spring Boot 4.1 con Maven, creato con start.spring.io. Le dipendenze di Maven vanno in un deposito nella cartella di prova, non nel tuo `~/.m2`.

  | Prova | Tempo |
  |---|---|
  | jdtls, installazione | 20 s |
  | jdtls, avvio | 1,2–2,7 s |
  | jdtls, primo import Maven con gli errori (scarica 62 MB di dipendenze) | 18 s |
  | jdtls, errori con il progetto già importato | 4,5 s in tutto |
  | jdtls, info / completamento / definizione | 0,4 s / 10–65 ms / 4–480 ms |
  | jdtls, riferimenti | 36–440 ms |
  | jdtls, sorgente di una classe di libreria | 107 ms |
  | Spring Boot, installazione | 4,9 s |
  | Spring Boot, completamento di `server.po` in `application.properties` | 2 s la prima volta, poi 5 ms |

- **Non provato con un progetto vero:** Gradle, che la prima volta scarica la sua distribuzione (circa 130 MB). Il codice è lo stesso di Maven.
- La prima ricerca dei riferimenti su una classe di libreria, con l'import appena finito, ha dato un errore interno di jdtls. Rifatta poco dopo ha funzionato.

**Note sulla 11.3**
- **Due server per Python**, come in VS Code con Pylance e l'estensione di Ruff:
  - **basedpyright** (da npm), la versione libera di Pyright, il motore di Pylance. Dà gli errori di tipo, i completamenti con gli import automatici, le info, le definizioni, i riferimenti, la rinomina e la struttura del file.
  - **Ruff** (archivio con impronta SHA-256). Dà gli avvisi di stile e i bug probabili con le regole del `pyproject.toml`, le correzioni (anche `source.fixAll.ruff` al salvataggio), il riordino degli import e la formattazione.
- **L'ambiente virtuale del progetto**, cercato come fa l'estensione Python di VS Code. In ordine:
  1. `python.defaultInterpreterPath` delle impostazioni;
  2. `.venv` (quello che crea uv), `venv` o `env`, purché siano davvero ambienti, cioè abbiano `pyvenv.cfg`;
  3. l'ambiente di Poetry nella sua cache.

  Così i pacchetti installati nel progetto si trovano. Senza nessun ambiente vale il `python3` del `PATH`.
- **Quanto controlla:** la modalità `standard`, quella del comando `pyright`. Pylance parte spento, basedpyright molto più severo. Il progetto può scegliere nel `pyproject.toml` (`[tool.basedpyright]`) o con `"basedpyright.analysis.typeCheckingMode"` nelle impostazioni (`off`, `basic`, `standard`, `strict`, `all`).
  - Su Hondana `standard` trova errori veri, per esempio un `CatalogIndex` che non rispetta il protocollo `CatalogLike`.
- **Prove con i server veri** (scaricati nella cartella di prova):

  | Prova | Tempo |
  |---|---|
  | basedpyright, installazione | 1,8 s |
  | basedpyright, primi errori su un file di prova | 0,45 s |
  | basedpyright, primi errori su `hondana/api.py` | 0,83 s |
  | basedpyright, info su `fastapi` (preso dal `.venv` del progetto) | 7 ms |
  | basedpyright, completamento: primo / dopo | 230 ms / 2 ms |
  | basedpyright, riferimenti | 9 ms |
  | Ruff, installazione | 0,6 s |
  | Ruff, errori | 8 ms |
  | Ruff, formattazione | meno di 1 ms |

- Ruff dà gli errori solo quando l'editor li chiede. Keel lo faceva già nella finestra; ora lo fa anche la prova automatica con i server veri.

**Note sulla 11.2**
- **Download controllati.** Ogni server scaricato ha la sua impronta SHA-256 scritta in Keel. Se il file scaricato non corrisponde, Keel lo butta e non installa niente. Vale anche per taplo e marksman della fase 6, che prima non avevano il controllo.
  - Le impronte sono quelle dei file per Mac Apple; sui Mac Intel non c'è niente con cui confrontare.
- **Archivi `tar.gz` e `zip`.** Ogni versione va in una cartella sua, così una nuova non si mescola con la vecchia. Il `tar` di macOS apre anche gli `zip`.
- **Programmi necessari:** Java, .NET e gli strumenti di Xcode.
  - Keel li controlla prima di installare il server: è inutile scaricare il server di Java senza Java.
  - **Java** si cerca in `JAVA_HOME`, nei JDK che macOS conosce, in quelli di Homebrew (anche senza il collegamento che Homebrew suggerisce) e nel `PATH`. Ci vuole la versione giusta: Keel lancia `java -version`. Su questo Mac trova Java 21 in 0,3 s.
  - **.NET** si cerca in `DOTNET_ROOT`, nel `PATH` e nelle cartelle dove lo mette il suo installatore; ci vuole un SDK della versione giusta.
  - **Gli strumenti di Xcode** ci sono se `xcode-select -p` indica una cartella che esiste. Senza di loro `/usr/bin/clangd` chiede solo di installarli.
- **Se manca qualcosa** Keel lo chiede una volta per finestra, per esempio: «Per il server Java serve Java 21, che non è installato». Con «Installa nel terminale» apre un terminale con il comando (`brew install openjdk@21`, `brew install --cask dotnet-sdk` o `xcode-select --install`). Quando ha finito, «Linguaggi: Riavvia i language server» fa ripartire i server.
- Il server riceve le variabili del programma trovato, come `JAVA_HOME` e la sua cartella in testa al `PATH`, così usa proprio quello.

**Note sulla 11.1**
- **Colori** per Python, Java, C, C++, C#, SQL, Makefile, XML, Dockerfile e `.properties`; Bash e zsh li avevano già.
  - Sette grammatiche erano già in gpui-component, da accendere.
  - XML, Dockerfile e `.properties` vengono da crate a parte.
  - C# era senza regole di colore e C++ ne aveva solo una parte: ora hanno quelle delle loro grammatiche, e C++ usa anche quelle di C, come fanno gli editor che lo supportano.
- **Nel Dockerfile** i comandi dopo `RUN` hanno i colori della shell. I file JSON, YAML, TOML e XML scritti dentro il Dockerfile hanno i loro colori.
- **Nei Makefile** tutti i target sono colorati come funzioni, come in VS Code; la grammatica colorava solo quelli standard (`all`, `clean`…).
- **Riconoscimento dei file**, con gli stessi identificativi di VS Code (che usano i server e le estensioni):
  - `Dockerfile`, `Containerfile` e `Dockerfile.dev`;
  - `Makefile`, `GNUmakefile`, `.mk`;
  - `pom.xml`, `.csproj`, `.svg`, `.plist` e gli altri file XML;
  - `.zshenv` e gli altri file di zsh.
- **`.h` vale come C++**, come in VS Code: `clangd` legge bene le intestazioni C in entrambi i casi.
- Nella barra di stato i nuovi linguaggi hanno il loro nome. Commenti con Cmd+/ e rientro dopo `:` in Python funzionavano già.

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
- **Rimasti dalla fase 2:**
  - sticky scroll;
  - minimappa;
  - pieghe da tastiera;
  - comandi sulle righe con più cursori;
  - trascinare le schede;
  - aprire più file insieme da `keel`;
  - regex e parola intera nella ricerca dentro il file (Cmd+F).

**Fatto quando**
- Usi Keel per qualche settimana al posto di Cursor senza dover tornare indietro.

**Sottofasi**

| Sottofase | Cosa | Stato |
|---|---|---|
| 12.1 | Affidabilità: registro locale dei crash e avviso alla ripartenza; prova di ripresa dopo un crash vero (`kill -9`) con le modifiche non salvate; salvataggi che rispettano permessi e collegamenti | fatta |
| 12.2 | Editor, i rimasti della fase 2: pieghe da tastiera, comandi sulle righe con più cursori, regex e parola intera in Cmd+F, più file insieme da `keel`, trascinare le schede | fatta |
| 12.3 | Sticky scroll e minimappa, disegnati da Keel | fatta |
| 12.4 | Impostazioni con interfaccia grafica ed editor delle scorciatoie | fatta |
| 12.5 | Importa da Cursor impostazioni, scorciatoie e progetti recenti | fatta |
| 12.6 | Rifinitura: animazioni, schermate vuote, coerenza di temi e icone, accessibilità di base, revisione dei testi in italiano e in inglese | fatta |
| 12.7 | Prestazioni: file e progetti enormi, sessione lunga senza perdite di memoria, misure finali contro Cursor; resoconto | fatta |

**Resoconto della fase 12**
- **Il criterio era:** usi Keel per qualche settimana al posto di Cursor senza dover tornare indietro. Questo lo decidi tu usandolo. La fase ha fatto quello che serviva perché sia possibile:
  - affidabilità (registro dei crash, chiusure brusche riconosciute);
  - i comandi di editor che mancavano;
  - impostazioni e scorciatoie con le loro schermate;
  - l'importazione da Cursor;
  - le misure finali.
- **Misure finali** (Keel ottimizzato, 18 novembre 2026):

  | Prova | Keel | Cursor |
  |---|---|---|
  | I due progetti di FilieraFacile aperti, due finestre | **112 MB** in 1 processo, primo fotogramma 455 ms | **2.886 MB** in 46 processi, così come lo stavi usando |
  | — di cui finestre, processo principale, GPU e servizi | 112 MB | circa 1.400 MB (le sole finestre 821 MB) |
  | — di cui estensioni | 0 (nessuna aperta) | 584 MB |
  | CPU a riposo, 15 secondi | 0,00 s | — |
  | File da 200.000 righe: apertura / un tasto | 383 ms / 1 ms | — |
  | Vai al file su tutta MiniPCProgram (32.809 file, 1,48 milioni contando gli ignorati) | indice in 0,64 s, ricerca in 61 ms | — |
  | Ricerca nel progetto di «booking» (8.100 risultati) | 0,74 s | 0,36 s (ripgrep di Cursor) |
  | Ricerca nel progetto di «useState» | 0,37 s | 0,37 s |

  - Il confronto con Cursor non è alla pari: Cursor aveva i file e i terminali che stavi usando. Non l'ho riavviato per misurarlo da vuoto, per non toccare le tue finestre. Già le sole finestre di Cursor pesano sette volte tutto Keel.
  - La ricerca di Keel era più lenta: 1,24 s su «booking». Leggeva riga per riga anche i file dove non c'era niente; ora un primo controllo sul file intero li scarta subito, e il tempo è sceso a 0,74 s. Resta circa il doppio di ripgrep quando i risultati sono tanti.
- **Sessione lunga senza perdite.** Ho comandato Keel dal collegamento IDE di Claude Code: 2.500 giri da 42 richieste, 105.000 in tutto, in 5 minuti. In ogni giro 40 file aperti a rotazione e una scheda di differenze creata e chiusa. La memoria va su e giù tra 137 e 198 MB e finisce più bassa di come era partita: nessuna perdita.
- **Sistemato durante le misure:**
  - **La minimappa ricalcolava le righe sul thread dell'interfaccia**: 46 ms su un file da 200.000 righe, ogni 250 ms mentre scrivi. Ora il calcolo gira in un thread di fondo e la scrittura resta a 1 ms a tasto.
  - **Annunci per Claude Code rimasti** dei Keel chiusi bruscamente: in `~/.claude/ide` ce n'erano 18 dei miei Keel di misura. Claude Code li avrebbe offerti in `/ide`. Ora Keel, partendo, toglie gli annunci di Keel il cui processo non esiste più, come fa l'estensione di VS Code; quelli degli altri editor restano.
- **Test:** 386 automatici, tutti verdi per tre volte di fila, più 26 da lanciare a mano.

**Le tue prove a mano della fase 12**
1. **Crash:** in un terminale lancia `kill -9` sul processo di Keel (`pgrep -x keel`), poi riapri Keel. Nella barra di stato compare «Keel non si è chiuso normalmente l'ultima volta», e le modifiche non salvate sono tornate.
2. **Pieghe:** in un file con funzioni, Cmd+Alt+[ e Cmd+Alt+], poi Cmd+K Cmd+0 e Cmd+K Cmd+J.
3. **Più cursori:** Alt+clic su tre righe, poi Cmd+/ e Alt+↓. Tutte le righe cambiano insieme, e Cmd+Z le rimette con un passo solo.
4. **Cmd+F:**
   - prova «Aa», «parola intera» e «.\*»;
   - con la regex `(\w+)_due` sostituisci con `$1-tre`;
   - scrivi una regex sbagliata come `(`: la barra la segnala in rosso.
5. **Schede:** trascina una scheda in un altro punto, poi in un altro gruppo (Cmd+\ per dividere). Dal terminale `keel a.ts b.ts` apre entrambi.
6. **Sticky scroll e minimappa:** scorri dentro una funzione lunga: la sua prima riga resta in alto, e un clic la riporta lì. Clicca e trascina sulla minimappa.
7. **Impostazioni (Cmd+,):**
   - cambia la dimensione del testo e il tema;
   - apri `settings.json`: i tuoi commenti ci sono ancora;
   - prova la ricerca in alto.
8. **Scorciatoie (Cmd+K Cmd+S):** cambia quella di un comando, provala subito, poi riportala com'era con la freccia indietro.
9. **Da Cursor:** dalla tavolozza «Preferenze: Importa impostazioni, scorciatoie e progetti recenti da Cursor». Leggi la domanda con i numeri e importa; i progetti di Cursor compaiono in File → Apri recenti.
10. **Aspetto:** guarda i temi Chiaro e Nero con i commenti più scuri o più chiari, e la barra di ricerca che scivola dentro. Con «Riduci movimento» acceso in Impostazioni di Sistema → Accessibilità → Schermo non deve muoversi.
11. **Settimane con Keel:** usalo al posto di Cursor e annota cosa ti fa tornare indietro: è il criterio della fase.

**Già fatto nelle fasi precedenti:** i salvataggi sicuri (file temporaneo, poi rinomina) e le copie delle modifiche non salvate, che tornano dopo un crash o Cmd+Q (fase 2.6).

**Note sulla 12.6**
- **Contrasto dei temi, misurato.** Un test calcola il contrasto WCAG di ogni tema: testo e testo attenuato almeno 4,5 (livello AA), ogni colore della sintassi almeno 4,5 sullo sfondo dell'editor, numeri di riga almeno 3. Sono state corrette otto tinte:
  - **Chiaro:** commenti da 4,1 a 5,5, link da 3,7 a 5,4, numeri di riga da 2,6 a 3,6;
  - **Nero:** commenti da 4,45 a 5,1, numeri di riga da 2,9 a 3,9;
  - **Mezzanotte** passava già.

  I tre temi hanno le stesse 60 voci di colore e la stessa sintassi.
- **Testi italiani e inglesi controllati da test:**
  - le parti da riempire (`{count}`, `{name}`…) sono le stesse nelle due lingue in tutti i 491 testi;
  - nessun testo è vuoto;
  - ogni comando di Keel ha il suo nome nella tavolozza («Mostra tutti i comandi» non l'aveva).
- **Schermate vuote:** le viste avevano già il loro messaggio (Problemi, Output, Porte, Controllo del codice, agente, ricerca). Ora ce l'hanno anche l'Esplora risorse su una cartella vuota («La cartella è vuota: crea un file con il tasto destro o con Cmd+N») e le scorciatoie quando la ricerca non trova niente.
- **Animazioni:** menu e popover di gpui-component entrano già con una dissolvenza. La barra di ricerca di Keel ora fa lo stesso (150 ms). Con «Riduci movimento» di macOS (Accessibilità → Schermo) Keel non anima niente.
- **Accessibilità, fin dove GPUI arriva:**
  - si usa tutto da tastiera;
  - Cmd+= ingrandisce il testo;
  - i temi rispettano il contrasto AA;
  - «Riduci movimento» viene rispettato.
- **Non ancora:** VoiceOver non vede l'interfaccia di Keel. GPUI prepara l'albero di accessibilità (AccessKit), ma la sua parte per macOS non lo passa al sistema: le funzioni ci sono e non fanno niente. Collegarlo vuol dire scrivere l'adattatore per macOS dentro GPUI, un lavoro a sé.
- La rifinitura visiva fine (spazi, allineamenti, icone una per una) si giudica guardando: resta alle tue prove a mano.

**Note sulla 12.5**
- **Nuovo comando** nella tavolozza: «Preferenze: Importa impostazioni, scorciatoie e progetti recenti da Cursor». Le estensioni si importavano già dalla fase 9.
- **Cosa legge**, senza mai scrivere nei file di Cursor:
  - `settings.json` e `keybindings.json` da `~/Library/Application Support/Cursor/User`;
  - i progetti recenti dal database di Cursor (`state.vscdb`), con il `sqlite3` di macOS in sola lettura.
- **Prima chiede**, per esempio: «Da Cursor: 16 impostazioni, 4 scorciatoie e 7 progetti recenti. Li importo?».
- **Non sovrascrive niente:**
  - le impostazioni che hai già in Keel restano;
  - le scorciatoie già presenti non si doppiano;
  - i progetti recenti di Keel restano per primi e quelli di Cursor vengono dopo;
  - le cartelle che non esistono più restano fuori.
- **Lasciate fuori** le impostazioni di Cursor stesso (`cursor.*`, `telemetry.*`, `update.*`) e il tema e le icone, che in Keel hanno nomi diversi.
- Le impostazioni importate valgono subito, e così le scorciatoie dei comandi di Keel. Quelle dei comandi delle estensioni valgono dalla prossima apertura.
- **Prova sui tuoi file veri**, solo in lettura e verso un Keel vuoto: entrerebbero 16 impostazioni (VSAuto, salvataggio automatico, minimappa…), 4 scorciatoie e 7 progetti recenti su 12 (gli altri 5 non ci sono più).

**Note sulla 12.4**
- **Scheda «Impostazioni» (Cmd+,)** con la pagina di impostazioni di gpui-component: le pagine a sinistra (Aspetto, Editor, File e finestre, Terminale, Linguaggi, Estensioni) e la ricerca in alto.
  - Contiene le 32 impostazioni che Keel legge davvero, con titolo e descrizione in italiano e inglese.
  - Ogni impostazione ha il suo controllo: interruttore, menu, numero o testo. Accanto c'è il pulsante per tornare al valore predefinito.
  - Una modifica si scrive subito nel tuo `settings.json` e vale subito nella finestra: dimensione del testo, a capo, lingua, tema e rientro si aggiornano al momento. Minimappa e sticky scroll valgono per i file aperti dopo.
  - Le impostazioni che sono oggetti o elenchi (`files.exclude`, `editor.codeActionsOnSave`…) hanno il pulsante «Modifica in settings.json».
  - Se il progetto cambia un'impostazione nelle sue, la descrizione lo dice.
  - «Apri settings.json» in alto apre il file; nella tavolozza c'è «Preferenze: Apri le impostazioni (JSON)».
- **`settings.json` con i commenti ora si può cambiare da Keel.** Prima Keel rifiutava, per non perderli. Ora cambia solo il testo della chiave toccata: commenti, ordine, righe vuote e rientro restano come li hai scritti. Vale anche per il tema scelto dalla tavolozza e per le impostazioni scritte dalle estensioni.
- **Scheda «Scorciatoie da tastiera» (Cmd+K Cmd+S)**, come quella di VS Code.
  - Elenca tutti i comandi di Keel con i loro tasti (scritti come sul Mac, per esempio ⇧⌘P), l'origine (Predefinita o Tua) e l'identificativo. Si cerca per nome, identificativo o tasto.
  - **Cambiare:** il pulsante a forma di matita, poi i tasti (anche due di fila, come ⌘K ⌘S) e Invio; Esc annulla.
  - La scorciatoia nuova va nel tuo `keybindings.json` nel formato di VS Code, insieme alla riga che toglie quella predefinita (`"-comando"`), lasciando intatti i commenti.
  - **Vale subito:** il tasto nuovo funziona e il vecchio smette, senza riavviare.
  - **Ripristinare:** la freccia indietro toglie le tue righe per quel comando e rimette il tasto predefinito.
- **Non ancora:** le impostazioni del progetto dalla schermata (per ora si scrivono le tue); le altre finestre aperte vedono le modifiche solo alla prossima apertura.

**Note sulla 12.3**
- **Sticky scroll:** in cima all'editor restano le prime righe dei blocchi in cui ti trovi (funzione, classe, `if`…), al massimo 5, con il numero di riga. Un clic porta a quella riga. I blocchi sono gli stessi delle pieghe, cioè quelli della grammatica del linguaggio.
- **Minimappa:** a destra di ogni editor c'è il file intero in piccolo, una riga di pixel per riga di codice, con la parte in vista ombreggiata. Un clic o un trascinamento ci porta l'editor.
  - Nei file lunghi le righe che cadono sullo stesso pixel si uniscono, quindi anche 200.000 righe costano al massimo un rettangolo per pixel di altezza.
  - Mentre scrivi, la minimappa si ricalcola al massimo 4 volte al secondo.
- **Si disegnano da sole:** sono due viste separate che seguono il loro editor, quindi lo scorrimento ridisegna solo loro, non tutta la finestra.
- Si spengono con `"editor.stickyScroll.enabled": false` ed `"editor.minimap.enabled": false`. Come in VS Code sono accese di base.
- **Diverso da VS Code:**
  - le righe ferme in alto non hanno i colori della sintassi;
  - la minimappa mostra la forma delle righe, non i caratteri (come l'opzione `renderCharacters: false` di VS Code).

**Note sulla 12.2**
- **Una copia di gpui-base in Keel** (`vendor/gpui-base`, la versione 0.9.1, scelta tua). Tre cose servivano a funzioni che gpui-component tiene private, quindi ho aggiunto poche funzioni pubbliche, segnate `KEEL:`.
  - L'elenco delle modifiche è in `vendor/gpui-base/KEEL.md`.
  - A una nuova versione si copia di nuovo il crate e si rifanno quelle modifiche.
- **Pieghe da tastiera**, con i tasti di VS Code:
  - Cmd+Alt+[ piega il blocco del cursore, Cmd+Alt+] lo riapre;
  - Cmd+K Cmd+0 piega tutto, Cmd+K Cmd+J riapre tutto.

  Si trovano anche nella tavolozza («Piega», «Espandi», «Piega tutto», «Espandi tutto»). Se il cursore finisce dentro una piega, va alla prima riga del blocco.
- **Comandi sulle righe con più cursori:** commenta, sposta, copia, elimina e inserisci righe ora valgono per ogni cursore, non solo per il principale.
  - I cursori sulla stessa riga o su righe vicine fanno un blocco unico, come in VS Code.
  - È un solo passo per Cmd+Z.
- **Barra di ricerca di Keel (Cmd+F, Cmd+Alt+F per sostituire)** al posto di quella di gpui-component.
  - **«Aa»** distingue maiuscole e minuscole, anche per le lettere accentate (prima solo per quelle inglesi).
  - **«Parola intera»** e **«.\*» (regex)**. Nella sostituzione con regex valgono `$1` e `${nome}`; se la regex è sbagliata, la barra dice perché, in rosso.
  - Mostra il risultato corrente sul totale («2/5»). Invio e Maiusc+Invio, o Cmd+G e Cmd+Maiusc+G, passano al successivo e al precedente; Esc chiude.
  - Cmd+Alt+C, Cmd+Alt+W e Cmd+Alt+R accendono le tre opzioni, come in VS Code.
  - Scrivendo, Keel seleziona il primo risultato; aprendo la barra con del testo selezionato su una riga, lo usa come ricerca.
- **Più file insieme:** `keel a.ts b.ts` apre entrambi, ognuno nella finestra del suo progetto. Funziona anche con Keel già aperto.
- **Schede da trascinare:** in un altro punto del gruppo o in un altro gruppo. Il file passa con il suo editor, cioè cursore, scorrimento e annulla. Un gruppo che resta vuoto si chiude. Le schede fissate restano davanti alle altre.
- Il trascinamento vero col mouse non si può provare nei test: ho provato lo spostamento che fa. Resta la tua prova a mano.

**Note sulla 12.1**
- **Registro dei crash, solo su questo Mac.** Se Keel va in errore, scrive `crashes/crash-<data>-<pid>.log` nella sua cartella dati. Dentro ci sono la versione di Keel e di macOS, il thread, il messaggio, il punto del codice e il backtrace. Tiene gli ultimi 20 registri. Niente viene mandato fuori.
- **Chiusure brusche.** Ogni Keel in esecuzione lascia un segno in `running/`, che toglie quando esce normalmente.
  - Se alla ripartenza c'è un segno di un processo che non esiste più, l'ultima volta Keel si è chiuso male: per un errore, per `kill -9` o perché il Mac si è spento.
  - La prima finestra lo dice nella barra di stato e nell'Output, con il percorso del registro se c'è.
  - Le modifiche non salvate tornano comunque dalle copie in `backups/`.
- **Uscite che contano come normali:** Cmd+Q, la chiusura dell'ultima finestra e il SIGTERM. Il SIGTERM è la richiesta educata che usano `kill` e il logout di macOS.
- **Provato con Keel vero:**
  - dopo `kill -9` il giro seguente lo segnala;
  - dopo un SIGTERM non segnala niente;
  - un errore vero, in un processo di prova, scrive il registro con il messaggio e il file giusto;
  - dopo `kill -9` non resta nessun language server orfano: escono da soli quando si chiude il loro canale.
- **Salvataggi:** collegamenti simbolici e permessi erano già rispettati. In più, un file scrivibile in una cartella non scrivibile ora si salva scrivendolo direttamente, come fa sempre VS Code. Prima falliva, perché il file temporaneo non si poteva creare.

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

**Per provarla come un'app vera** (18 novembre 2026): `scripts/make-dmg.sh` costruisce Keel.app con la sua icona e la mette in `target/release/Keel-<versione>.dmg`, insieme alla scorciatoia ad Applicazioni da cui trascinarla. L'app si apre anche da Finder con «Apri con → Keel», per file di testo, codice e cartelle.
- È firmata solo per questo Mac (firma «ad-hoc»), non da uno sviluppatore registrato da Apple. Dal DMG fatto qui si apre normalmente.
- Una copia scaricata da internet, la prima volta, va aperta con il tasto destro → Apri, oppure da Impostazioni di Sistema → Privacy e sicurezza → «Apri comunque».
- Per firmarla e farla controllare da Apple serve un account sviluppatore (99 dollari l'anno): è la pubblicazione vera, ancora da decidere.

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
- ~~**Bozze della fase 1**~~ fatte in HTML statico; hai scelto la bozza D, che unisce B e C.
- ~~**Libreria per le lingue**~~ deciso nella 1.3: una tabella di testi per lingua scritta in Rust, così il compilatore controlla che nessun testo manchi. Con più lingue si potrà passare a Fluent.
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
