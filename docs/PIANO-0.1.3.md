# Keel 0.1.3 — proposta

*Scritto il 26 novembre 2026, sulla 0.1.2. Stato: **approvata il 26 novembre** («Sì, parti»); **sottofasi 1–5 fatte il 26 e 27 novembre**, su `develop`, con le prove a mano ancora da fare (vedi «Com'è andata»); resta l'uscita (sottofase 6).*

Da dove viene:
- **le due richieste dell'autore** del 26 novembre: le chat degli altri progetti nella finestra dove si lavora, e le icone di serie come Material Icon Theme;
- **il confronto con l'estensione ufficiale di Claude Code per VS Code**, come la descrive oggi la sua pagina, perché il pannello di Claude è la parte di Keel che si usa di più;
- **le ultime versioni dei concorrenti:** VS Code 1.146, 1.147 e 1.148 (11, 18 e 25 novembre), Zed dalla 1.27 alla 1.30 (dal 4 al 25 novembre);
- **i difetti e i limiti già noti**, da `HANDOFF.md`, `DETTAGLI.md` e `IDEE.md`.

I pesi sono quelli di `IDEE.md`: *piccolo* (un giorno o meno), *medio* (qualche giorno), *grande* (una o due settimane).

---

## In breve

| # | Cosa | Peso | Da dove |
|---|---|---|---|
| 1 | Le chat degli altri progetti si aprono qui | piccolo | richiesta dell'autore |
| 2 | Icone di serie come Material Icon Theme | medio | richiesta dell'autore |
| 3 | Dettatura nel pannello di Claude | piccolo, o medio | estensione di Claude Code (⌘D) |
| 4 | Riportare il codice a un messaggio, e dividere una chat | medio | estensione di Claude Code |
| 5 | Il piano in una scheda, con i commenti sulle righe | medio | estensione di Claude Code, Warp, Xcode 27 |
| 6 | Le piccole cose del pannello | piccolo ciascuna | estensione di Claude Code, VS Code 1.148 |
| 7 | Anteprima web dentro Keel | grande | VS Code 1.147, Cursor 3.7 |
| 8 | Git: cosa cambia rispetto a `main`, file trascinati, zoom | piccolo ciascuna | Zed 1.28 e 1.29 |
| 9 | Difetti noti, numeri rimisurati, sito | piccolo ciascuna | `HANDOFF.md`, `DETTAGLI.md`, `IDEE.md` |

Il punto 7 è il più grande. Se la 0.1.3 deve uscire presto, può passare alla 0.1.4 senza toccare il resto. 🔸

---

## 1. Le chat degli altri progetti si aprono qui

**Oggi.** Nel pannello di Claude le chat sono divise per progetto. Un clic su una chat di un altro progetto (per esempio MiniPCProgram, da una finestra di filierafacile) apre quel progetto in una finestra sua, o la porta davanti, e continua lì la chat (`open_agent_chat` in `agent_session.rs`, con `windows::open_then`).

**Domani.**
- La chat si apre **nel pannello della finestra dove sei**, come quelle del progetto aperto.
- **Claude continua a lavorare nella cartella della chat**, non in quella della finestra: la storia della chat parla dei file di quella cartella, e Claude Code tiene le chat per cartella. Il protocollo lo permette già: ogni sessione ha la sua cartella (`session/load` con il `cwd` della chat), anche con un solo adattatore per finestra.
- **L'intestazione lo dice:** «in MiniPCProgram», con un pulsante per aprire quel progetto nella sua finestra, per chi lo vuole ancora.
- **Seguono la cartella della chat:**
  - la rilettura della chat quando cambia fuori da Keel, che oggi cerca il file nella cartella della finestra;
  - «Continua nel terminale», che deve partire da quella cartella, perché `claude --resume` cerca la chat lì;
  - le modifiche di Claude, che possono essere fuori dal progetto: si aprono comunque in una scheda di questa finestra (`open_file` apre qualunque percorso).
- **«+», una chat nuova,** torna alla cartella del progetto aperto.

**Prove:** l'agente finto dei test apre una chat in un'altra cartella e riceve quella cartella; una modifica fuori dal progetto finisce nell'elenco delle modifiche e si apre in una scheda; la chat resta nel pannello e non si apre nessuna finestra.

## 2. Icone di serie come Material Icon Theme

**Oggi.** Senza estensioni Keel ha una ventina di simboli generici, colorati per estensione del file (`file_icon` in `explorer.rs`), e le cartelle sono tutte uguali. Le icone vere arrivano solo installando un'estensione: sul Mac dell'autore c'è Material Icon Theme 5.40.0, attivo.

**Domani.**
- **Keel porta con sé le icone di Material Icon Theme**, già attive alla prima apertura:
  - ogni file per nome e per estensione: `.tsx` con l'atomo di React, `.service.ts`, `package.json`, `Dockerfile`, `.env`, `Cargo.toml`…;
  - ogni cartella per nome, aperta o chiusa: `src`, `components`, `public`, `.github`, `node_modules`, `.claude`…;
  - le varianti per il tema chiaro.
- **Ovunque si vede un file:** albero, schede, Controllo versione, Cerca, Cmd+P, Problemi, differenze, pannello di Claude (i file toccati e quelli aggiunti con @), benvenuto e progetti recenti.
- **I simboli di oggi restano come scelta,** «Icone semplici», e i temi di icone delle estensioni continuano a funzionare.
- **Chi ha già l'estensione** non vede cambiare niente: le icone sono le stesse.

**Perché si può.** Material Icon Theme ha la licenza MIT, che permette di metterlo dentro un'app chiedendo solo di portare con sé il suo avviso. Sono 1.256 icone SVG, circa 1 MB, più l'elenco di 443 KB che dice quale icona va a quale file: Keel lo legge già in 25 ms (fase 9.3). Cosa serve:
- l'avviso della licenza in `THIRD-PARTY-LICENSES.txt`: `scripts/third-party-licenses.py` oggi legge solo `Cargo.lock` e deve imparare ad aggiungere i file che non vengono da lì;
- uno script che rifà la copia da una versione nuova di Material Icon Theme;
- il ringraziamento nel README e nel sito.

**In più, se piace:** ogni progetto con la sua icona (la favicon o il logo che ha nella cartella) nei progetti recenti, nel benvenuto e nei gruppi delle chat, come fa JetBrains. Piccolo.

**Il sito** mostra una copia di Keel (`src/keel/` di KeelSite): deve avere le stesse icone.

## 3. Dettatura nel pannello di Claude

L'estensione ufficiale ha la dettatura (⌘D). In Keel un microfono nel campo di Claude, e ⌘D quando il cursore è lì (fuori dal campo ⌘D resta «seleziona la prossima occorrenza», come in VS Code).
- **Prima scelta:** la dettatura di macOS, che sui Mac con Apple Silicon riconosce l'italiano sul Mac stesso e non chiede di installare niente. Va provato che il campo di testo di GPUI la riceva bene.
- **Se non basta:** il riconoscimento vocale di Apple (framework Speech), sempre sul Mac. Chiede il permesso per il microfono; con un'app senza la firma di Apple macOS potrebbe richiederlo dopo ogni aggiornamento.

## 4. Riportare il codice a un messaggio, e dividere una chat

Come nell'estensione: passando su un tuo messaggio, «Riporta il codice a qui», «Continua da qui in una chat nuova», oppure tutti e due.
- **Dividere una chat:** l'adattatore che Keel usa (0.107.3) ha già `session/fork`.
- **Riportare il codice:** Keel tiene già una copia di ogni file prima della prima modifica di Claude nella chat (`snapshots` in `agent_panel.rs`); le copie diventano una per messaggio.
- **Il limite, uguale a quello di Claude Code:** i file cambiati da un comando (`npm install`, `sed`…) non tornano indietro.
- Zed ha tolto i suoi punti di ripristino nella 1.27, perché nei progetti grandi rallentavano o bloccavano l'agente. Keel copia solo i file che Claude scrive, quindi il costo resta piccolo; va comunque misurato su un progetto grande.

## 5. Il piano in una scheda, con i commenti sulle righe

- **In modalità Piano,** quando Claude propone il piano, il piano si apre come documento Markdown in una scheda. Si commentano le righe, poi «Procedi», oppure «Rivedi il piano» manda tutti i commenti a Claude insieme.
- **Gli stessi commenti nelle modifiche di Claude:** nelle differenze si commentano le righe, e i commenti partono tutti insieme. È la revisione di Warp e di Xcode 27, il punto 2 di `IDEE.md`. Accetta e rifiuta per blocco ci sono già (giro 9 di `DETTAGLI.md`).

## 6. Le piccole cose del pannello

| Cosa | Oggi |
|---|---|
| **Esc ferma Claude**, come nell'estensione | Esc chiude solo i menu |
| **Scrivere mentre Claude lavora:** il messaggio parte appena il turno finisce | l'invio è bloccato finché Claude non ha finito |
| **La chat aperta torna** quando riapri Keel | si riparte dalla pagina iniziale |
| **«Copia la risposta»** sui messaggi di Claude | manca |
| **Rinominare e archiviare una chat** (l'adattatore ha `_session/rename` e `_session/archive`) | si può solo eliminare |
| **«Aggiungi a Claude»** col tasto destro sui file e sulle cartelle dell'albero, e sulla selezione nell'editor | si trascinano nella chat |
| **Il nome del modello leggibile**, «Opus 5.5», anche quando Claude Code dà l'id | può comparire `claude-opus-5-5` |
| **Il Mac non va in stop mentre Claude lavora**, solo durante un turno, con un'impostazione per spegnerlo (come VS Code 1.148) | il Mac può addormentarsi a metà lavoro |

## 7. Anteprima web dentro Keel

- **Una scheda con la pagina del progetto** (`localhost:3000`) accanto al codice. Si apre da Porte («Apri in Keel»), dall'avviso «La porta 3000 è aperta» o dalla palette.
- **Dentro:** indirizzo, avanti e indietro, ricarica, «Apri nel browser», larghezza da telefono e da tablet, l'Inspector di Safari.
- **«Mostra a Claude»:** un'immagine della pagina va nella chat, per dire «questo bottone è storto». È quello che VS Code fa con «Attach screenshots» e il suo browser integrato (1.147).
- **Come:** il WebKit di macOS (`WKWebView`) sopra la finestra di Keel, esattamente come fa Sail (`crates/webkit`, `mac.rs`). Le parti difficili, cioè posizione, angoli arrotondati, tastiera e clic, lì sono già risolte e provate. I processi di WebKit esistono solo mentre un'anteprima è aperta: senza anteprime Keel resta un processo solo.
- **Apre la strada** a due fasi opzionali di `PIANO.md`: i diagrammi Mermaid nell'anteprima Markdown e le pagine web delle estensioni.

## 8. Git

- **«Cosa cambia rispetto a main»:** tutti i file cambiati sul ramo rispetto al ramo di base, prima di aprire una pull request (Zed 1.29).
- **Trascinare un file dalla vista Git** nel terminale (scrive il percorso) o nell'editor (lo apre) (Zed 1.28).
- **⌘ con la rotella** ingrandisce il testo del terminale e delle differenze (Zed 1.28).

## 9. Difetti noti, numeri, documenti e sito

- **Difetti e mancanze già noti:**
  - i test di un crate in una sottocartella senza workspace non partono;
  - Cmd+P: la freccia a destra apre il file in secondo piano e lascia aperto l'elenco;
  - «Non mostrare più» negli avvisi che si ripetono;
  - il messaggio del commit in una scheda grande (Maiusc+Esc).
- **Il confronto rimisurato.** Le cifre del README e del sito (265 MB contro 1.014 di VS Code e 1.320 di Cursor) sono del 22 novembre, prima della correzione della 0.1.2: da fermo ora Keel scende a circa 157 MB. Vanno rimisurati tutti e tre gli editor insieme, con lo stesso metodo, poi cambiati:
  - il README e il suo grafico;
  - `src/site.ts` del sito;
  - il video di 40 secondi (`film/render.sh` di KeelSite), che l'autore deve poi ricaricare su GitHub.
- **Il sito:**
  - la domanda «Cosa manda in rete?» non dice dei remoti Git (il fetch ogni pochi minuti) né di Claude, che la pagina della privacy invece elenca;
  - la pagina delle novità con la 0.1.3.
- **I documenti:** allineati il 26 novembre: firma di Apple, licenza, la correzione uscita con la 0.1.2, il numero dei test, lo stato di `PIANO.md`, i dettagli di `PUBBLICAZIONE.md` e l'editor dei conflitti in `IDEE.md`.

---

## L'ordine proposto

Una sottofase alla volta, ognuna chiusa da test automatici, immagini di `scripts/snapshots.sh`, le prove a mano per l'autore e un commit su `develop`.

1. **Claude, subito:** le chat degli altri progetti qui (punto 1), le piccole cose del pannello (6), la dettatura (3).
2. **Le icone** (2).
3. **Claude, più a fondo:** riportare il codice e dividere le chat (4), il piano e i commenti (5).
4. **L'anteprima web** (7).
5. **Git e difetti** (8, e i difetti del 9).
6. **L'uscita:** il confronto rimisurato, i documenti, il sito, `CHANGELOG.md` e `docs/CHANGELOG.it.md`, poi il rilascio con `scripts/release.sh` (VPN accesa, il codice di verifica lo scrive l'autore), il push di `develop` e del tag quando lo chiede l'autore, e la pagina delle novità del sito.

## Com'è andata

### Sottofase 1 — Claude, subito (26 novembre)

**Fatto**, un commit per argomento su `develop`:
- **Le chat degli altri progetti si aprono qui** (`fc535d8`). Ogni chat ha la sua cartella (`chat_folder` nel pannello): `session/load` la riceve, e la seguono la rilettura della chat, il suo file e «Continua nel terminale» (`cd` nella cartella, oppure un terminale nuovo in basso se a destra Claude Code è al lavoro). Sopra la chat una fascia dice «Claude lavora in …», con «Apri il progetto». «+» torna a questo progetto. `windows::open_then` non serviva più ed è stato tolto.
- **Esc ferma Claude, i messaggi aspettano il turno, la chat torna** (`f8c2086`, `35c8025`). Un messaggio scritto mentre Claude lavora compare sopra il campo («Parte quando Claude ha finito», con la X per toglierlo) e parte alla fine del turno; più messaggi partono come uno, con i testi uniti. La sessione del progetto ricorda la chat aperta e la sua cartella (`claude_chat` in `WindowState`): riaprendo la finestra, la chat torna quando il pannello di Claude si vede, e Claude riparte.
- **Rinomina, copia, «Aggiungi a Claude»** (`931adbd`). La matita accanto al cestino rinomina una chat di Claude Code con `session/set_title`; un pulsante sotto ogni risposta la copia in Markdown; nel menu col tasto destro dell'albero «Aggiungi a Claude» allega il file, o i file scelti insieme.
- **Nome del modello leggibile e Mac sveglio** (`fcc3789`). `readable_model` in `session_meta.rs` (`claude-opus-5-5` → «Opus 5.5»; gli altri nomi restano). Durante un turno Keel tiene sveglio il Mac con `NSProcessInfo` (`mac::stay_awake`), con `keel.claude.keepAwake` (acceso di serie) in Impostazioni › File e finestre, gruppo «Claude».
- **Dettatura** (`8f4d523`). Il microfono nel campo e ⌘D quando il cursore è lì avviano la dettatura di macOS (`startDictation:` all'applicazione): niente da installare, nessun permesso nuovo per Keel.
- **Un difetto trovato con le foto** (`35c8025`): due messaggi scritti mentre Claude si avviava si sovrascrivevano e partiva solo l'ultimo; ora partono insieme. E un messaggio in coda aspetta 150 ms dopo la fine del turno, perché gli ultimi pezzi della risposta arrivano a Keel separati dalla fine del turno e prima comparivano dopo il messaggio.

**Diverso dal piano:**
- **Archiviare le chat non si può:** l'adattatore dà `_session/archive` solo ai programmi di JetBrains (AIR). Resta eliminarle.
- **«Aggiungi a Claude» nel menu dell'editor non c'è:** quel menu è di gpui-component, e per aggiungere una voce andrebbe rifatto tutto. Per la selezione restano `@` (la prima voce è la selezione) e il comando «Claude Code: Aggiungi il file o la selezione alla conversazione».

**Controlli:** 594 test, tutti verdi, più 28 da lanciare a mano (nell'app 332, più 6); clippy pulito; le foto di `scripts/snapshots.sh` (`claude-elsewhere`, `claude-queued` e le altre del pannello) guardate. Il test del Mac sveglio chiede a macOS con `pmset` se l'attività c'è davvero.

**Le tue prove a mano:**
1. Da una finestra di un progetto, nel pannello di Claude, apri una chat di un altro progetto: si apre lì, con «Claude lavora in …». Chiedile «in che cartella sei?».
2. «Apri il progetto» apre quel progetto in una finestra sua, e la chat resta dov'è.
3. Su quella chat, «Continua nel terminale»: Claude Code riparte nella cartella giusta.
4. Mentre Claude lavora, scrivi un altro messaggio e premi Invio: aspetta sopra il campo e parte alla fine. Esc nel campo ferma Claude.
5. Chiudi Keel con una chat aperta e riaprilo: la chat torna.
6. Rinomina una chat con la matita; copia una risposta; tasto destro su un file dell'albero → «Aggiungi a Claude».
7. Il nome del modello nel campo dice «Opus 5.5», non l'id.
8. ⌘D nel campo, o il microfono, poi parla: il testo deve comparire nel campo. Se macOS chiede di attivare la dettatura, si attiva in Impostazioni di Sistema › Tastiera. Questa è l'unica cosa che da qui non si è potuta provare.
9. Mentre Claude lavora, nel Terminale `pmset -g assertions | grep Keel` mostra «Claude is working in Keel»; a turno finito sparisce.

### Sottofase 2 — Le icone (26 novembre)

**Fatto** (`daaa538`):
- **Material Icon Theme 5.40.0 dentro Keel**, in `crates/keel/icons/material` (le 1.256 icone, l'elenco `dist/material-icons.json` e la licenza MIT), attivo dalla prima apertura: ogni file per nome ed estensione, ogni cartella per nome, aperta o chiusa, con le varianti per i temi chiari. Si vede nell'albero e nelle cartelle fissate in alto, nelle schede, nella vista Git, nella ricerca, in Cmd+P, in Problemi e nel pannello di Claude (menu `@` e file allegati). I simboli di prima restano come «Icone semplici»; un tema di icone di un'estensione, scelto, vince ancora.
- **L'elenco si legge una volta sola** per tutte le finestre, da un thread all'avvio (`preload_keel_icons` in `contributions.rs`); i percorsi del tema sono messi in ordine (`dist/./../icons` → `icons`) in `extensions/src/icon_theme.rs`. Le icone viaggiano in `Keel.app/Contents/Resources/icons` (`scripts/bundle-macos.sh`), e l'avviso della licenza è in `THIRD-PARTY-LICENSES.txt` (`BUNDLED` in `scripts/third-party-licenses.py`).
- **Ogni progetto con la sua immagine** (`project_icons.rs`): l'icona che tiene per sé (`.keel/icon.svg`), quella di un'app web, la favicon o un logo, altrimenti la cartella; nei progetti recenti della pagina iniziale e nelle schede delle chat di Claude.
- **Per una versione nuova** delle icone: `scripts/update-material-icons.sh <cartella dell'estensione>` copia elenco, icone e licenza, e aggiorna la versione negli avvisi (scritto il 27 novembre, insieme al ringraziamento nel README).

**Le tue prove a mano:**
1. Apri un progetto web (Hospitality o filierafacile): i `.tsx` hanno l'atomo di React, `package.json`, `.env`, `Dockerfile` le loro icone; `src`, `components`, `public`, `.github` le loro cartelle, diverse aperte e chiuse.
2. Col tema Chiaro le icone usano le varianti chiare.
3. Palette › «Preferenze: Tema icona file» › «Icone semplici» rimette i simboli di prima.
4. Nella pagina iniziale i progetti recenti hanno la loro immagine (la favicon di un'app web, per esempio).

### Sottofase 3 — Claude, più a fondo (26 e 27 novembre)

**Fatto:**
- **Riportare il codice a un messaggio** (`a549875`). Prima della prima modifica di ogni turno Keel tiene una copia di ogni file (`checkpoints` in `agent_panel.rs`). Passando su un tuo messaggio, «Riporta il codice a qui» chiede una volta e rimette ogni file cambiato da quel turno in poi; un file creato da Claude va nel Cestino. Come in Claude Code, quello che hanno cambiato i comandi non torna.
- **Dividere una chat** (`a549875`). Su una risposta, «Continua da qui in una chat nuova» copia la chat fino a lì con `session/fork` (il punto è il `messageId` della risposta, che ora il pannello tiene); da un tuo messaggio la copia si ferma prima, e il messaggio aspetta nel campo.
- **Il piano in una scheda** (`65c01ba`). Quando Claude esce dalla modalità Piano e chiede «Ready to code?», il piano si apre come scheda Markdown dell'editor. Il riquadro del permesso offre «Rivedi con le mie note»: il piano come si legge ora torna a Claude, che continua a pianificare; «Apri il piano» se la scheda è stata chiusa. Il sì chiude la scheda.
- **I commenti sulle modifiche di Claude** (`9bba40d`, 27 novembre). Nella scheda di una modifica di Claude, il numero di una riga, o il 💬 di un blocco, apre una finestra per il commento (⌘↩ lo tiene, vuoto si toglie). I commenti si vedono sotto le loro righe, come su GitHub, e «Manda a Claude» (nella scheda e nel riquadro delle modifiche del pannello) li manda tutti in un messaggio, ognuno con il file, le righe e il codice citato. Rifiutare una modifica toglie i suoi commenti. Anche le note delle pull request ora stanno sotto la riga, così il codice tiene la sua larghezza.

**Diverso dal piano:**
- **Le note sul piano si scrivono nel piano stesso,** che è un documento dell'editor: non ci sono commenti separati sulle righe del piano. «Rivedi con le mie note» manda il piano come si legge, con le note dentro.
- **I commenti sulle modifiche** si mettono su una riga o su un blocco, non su una selezione: la vista delle differenze di Keel non è un editor.

**Le tue prove a mano:**
1. Chiedi a Claude due modifiche in due messaggi. Sul primo messaggio, «Riporta il codice a qui»: i file tornano com'erano prima del primo turno.
2. Su una risposta, «Continua da qui in una chat nuova»: la chat nuova ha la storia fino a lì.
3. In modalità Piano chiedi qualcosa di grande: il piano si apre in una scheda; cambia una riga e aggiungi una nota, poi «Rivedi con le mie note».
4. Dopo una modifica di Claude, apri la sua scheda: clic sul numero di una riga, scrivi, ⌘↩; il 💬 di un blocco; poi «Manda a Claude» e guarda il messaggio partito.

### Sottofase 4 — L'anteprima web (27 novembre)

**Fatto** (`47018f9`):
- **Una pagina web in una scheda.** Il nuovo crate `web` mette il WebKit di macOS (`WKWebView`) sopra la finestra, come fa Sail; Keel disegna la barra sopra e lascia lo spazio sotto, dove la pagina sta mentre la sua scheda si vede, e si nasconde quando è aperta una finestra di dialogo. Si apre da Porte («Apri in Keel») o dalla palette («Anteprima: Apri la pagina del progetto», con le porte aperte tra cui scegliere).
- **La barra:** avanti, indietro, ricarica, l'indirizzo, la larghezza di un tablet e di un telefono, «Mostra a Claude» (un'immagine della pagina va col messaggio dopo), l'Inspector di WebKit e «Apri nel browser».
- `objc2-web-kit` è entrato negli avvisi delle licenze.

**Diverso dal piano:** la pagina non si apre dall'avviso «La porta 3000 è aperta», che è una scritta nella barra di stato e non un avviso con pulsanti: si apre da Porte e dalla palette.

**Le tue prove a mano:**
1. In un progetto web lancia `pnpm dev` nel terminale; in Porte, «Apri in Keel»: la pagina si apre in una scheda.
2. Naviga, poi avanti e indietro; ricarica; la larghezza del telefono e del tablet; l'Inspector; «Apri nel browser».
3. «Mostra a Claude», poi scrivi «cosa vedi?» nel pannello: l'immagine parte col messaggio.
4. Con la pagina aperta, apri Cmd+P o cambia scheda: la pagina sparisce e torna.

### Sottofase 5 — Git e difetti (27 novembre)

**Fatto:**
- **Cosa cambia rispetto a main** (`d0a1716`): «Git: Cosa cambia rispetto al ramo di base», nella palette e nel menu della vista Git, apre una pagina con tutti i file cambiati dal ramo da quando si è staccato dal ramo di base (`origin/HEAD`, altrimenti `main` o `master`), come li mostrerebbe una pull request.
- **File trascinati dalla vista Git, e ⌘ con la rotella** (`a95f5e2`): un file cambiato si trascina come uno dell'albero, nel terminale (scrive il percorso), nell'editor (si apre) o nella chat di Claude; ⌘ con la rotella o il trackpad ingrandisce il testo del terminale, da 8 a 40 punti.
- **I test di un crate in una sottocartella** (`9d327e5`): partono dalla cima del progetto con `--manifest-path`, che il crate sia in un workspace o no.
- **Cmd+P** (`164948c`): → nel campo vuoto, o ⌥↩ dovunque, apre il file evidenziato in una scheda che resta e lascia aperto l'elenco; in fondo all'elenco lo dice «→ per aprire dietro».
- **«Non mostrare più»** (`99c9977`) sugli avvisi che si ripetono: i messaggi delle estensioni, i programmi del terminale che chiamano, i comandi lunghi finiti e Claude che aspetta mentre Keel è dietro. Gli avvisi nascosti restano in `keel.notifications.hidden`; «Avvisi: mostra di nuovo quelli nascosti» li fa tornare tutti.
- **Il messaggio del commit in grande** (`25e754f`): Maiusc+Esc nel messaggio, o ⤢ sotto, apre lo stesso testo in una finestra grande, tenuta allineata col campo; ⌘↩ fa il commit da lì.

**Diverso dal piano:** il messaggio del commit si scrive in una finestra grande sopra l'editor, come in Zed, non in una scheda: resta legato al campo della vista Git, e Claude (✦) e il commit leggono lo stesso testo.

**Le tue prove a mano:**
1. Su un ramo con qualche commit, palette → «Git: Cosa cambia rispetto al ramo di base».
2. Trascina un file dalla vista Git nel terminale, nell'editor e nella chat di Claude.
3. ⌘ con la rotella sul terminale.
4. Cmd+P, frecce, → su un file: si apre dietro e l'elenco resta; scrivi qualche lettera e ⌥↩.
5. Su un avviso che torna (per esempio un comando lungo finito con Keel dietro), «Non mostrare più»; poi la palette → «Avvisi: mostra di nuovo quelli nascosti».
6. Nella vista Git scrivi un messaggio e premi Maiusc+Esc; scrivi qualche riga, poi ⌘↩.

**Controlli delle sottofasi 2–5:** 607 test, tutti verdi, più 28 da lanciare a mano (nell'app 343, più 6); clippy pulito; gli avvisi delle licenze aggiornati; le foto di `scripts/snapshots.sh` guardate, con quattro nuove: `commit-big` (la finestra grande del messaggio), `claude-review` e `claude-review-tab` (i commenti sulle modifiche di Claude). Le foto hanno trovato due difetti, corretti prima del commit: la nota di un commento accanto alla riga schiacciava il codice in una colonna di una lettera (ora va sotto la riga), e il ⌘↩ dentro il pulsante blu non si leggeva.

### Sottofase 6 — L'uscita (27 novembre, in corso)

**Il confronto rimisurato**, con lo stesso metodo del 22 novembre (stesso Mac, progetto Keel con `layout.rs` aperto, profilo vuoto, nessuna estensione, language server spenti, media di tre avvii), sulla versione release della 0.1.3:

| | Keel | VS Code 1.125 | Cursor 3.30 |
|---|---|---|---|
| Memoria di tutti i processi, dopo 40 s | 180 MB (171, 193, 175) | 748 MB (736, 828, 680) | 1.344 MB (1.575, 1.239, 1.217) |
| Avvio | 448 ms | 1.038 ms (due avvii su tre: il primo non ha scritto i tempi) | 1.636 ms |
| App su disco | 144 MB | 1.096 MB | 1.436 MB |
| Processi | 1 | 8 | 15 |

Il 22 novembre erano 265, 1.014 e 1.320 MB. Keel scende per la correzione della 0.1.2; VS Code risulta più leggero e più lento di allora, con la stessa versione: sono le sue variazioni da un avvio all'altro, e i tempi d'avvio ora vengono da avvii a parte, perché `--prof-append-timers` chiude l'editor appena scrive i tempi. Su disco Keel cresce di 7 MB per le icone. Le cifre nuove sono nei due README, nel grafico `assets/readme/comparison.svg`, in `src/site.ts` e nel film di KeelSite (`film/Film.tsx`), che va ridisegnato con `film/render.sh` e ricaricato su GitHub dall'autore.

**Fatto anche:** `CHANGELOG.md` e `docs/CHANGELOG.it.md`; i README (pannello di Claude, aree, privacy con le pagine web, stato, crate `web`, ringraziamento a Material Icon Theme); `HANDOFF.md`, `IDEE.md` e `DETTAGLI.md`; nel sito la pagina delle novità con la 0.1.3 (build 5), la domanda «Cosa manda in rete?» con i remoti Git e Claude, e le icone di Material nella copia di Keel. Le note del rilascio sono in `target/release-notes-0.1.3.md`.

**Resta:** `scripts/release.sh 0.1.3 target/release-notes-0.1.3.md` con la VPN accesa (il codice di verifica lo scrive l'autore), poi, quando lo chiede l'autore, il push di `develop` e del tag, e di KeelSite su `main` per pubblicare la pagina delle novità.

## Dopo la 0.1.3

Restano in `IDEE.md`, per la 0.1.4 o dopo:
- **più chat di Claude insieme,** ognuna nel suo worktree, con un elenco di chi aspetta te, chi lavora e chi ha finito: Cursor 3, l'Inbox di VS Code (1.146–1.148), l'elenco delle sessioni dell'estensione con i pallini blu e arancioni;
- **Swift** (con `sourcekit-lsp`, che c'è già con Xcode, e il debug con lldb-dap, che Keel usa già) e **Go**;
- **i colori dal language server** (semantic tokens);
- **l'editor dei conflitti a tre colonne**;
- **terminale e pannelli in finestre loro**;
- **VoiceOver**;
- **lo stile personale** (`style.css`).

## Decisioni

Prese col via del 26 novembre, che ha accettato le scelte proposte:
- l'anteprima web (punto 7) entra nella 0.1.3;
- le icone di Material Icon Theme sono quelle di serie per tutti, fin dalla prima apertura;
- ogni progetto ha la sua icona nei recenti e nei gruppi delle chat (punto 2, «In più»).

## Fonti

- [Claude Code per VS Code, la pagina ufficiale](https://code.claude.com/docs/en/vs-code)
- [VS Code 1.146](https://code.visualstudio.com/updates/v1_146), [1.147](https://code.visualstudio.com/updates/v1_147) e [1.148](https://code.visualstudio.com/updates/v1_148)
- [Zed, le versioni stabili](https://zed.dev/releases/stable), dalla [1.27.0](https://zed.dev/releases/stable/1.27.0) alla 1.30.2
- [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme), licenza MIT
