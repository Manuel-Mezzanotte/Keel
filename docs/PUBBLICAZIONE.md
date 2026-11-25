# Keel — piano della messa online

*Scritto il 23 novembre 2026, con le decisioni dell'autore dello stesso giorno. Stato: **tutto fatto il 23 novembre.** keel.mezzalab.eu è online dal mini PC (KeelSite `d15db7b`) e **Keel 0.1.0 (build 2)** si scarica dal sito, con lo script e con il DMG (commit `596c6d0`, tag `v0.1.0`).*

Lo scopo è rendere Keel scaricabile da chiunque da **keel.mezzalab.eu** e fare in modo che si aggiorni dall'app stessa. Per ora **senza pagare l'account sviluppatore di Apple**. La firma di Apple si potrà aggiungere dopo senza rifare niente.

---

## Le decisioni prese

### Aggiornamenti dentro Keel
- **Nelle Impostazioni**, nella pagina Keel o in una pagina «Aggiornamenti»:
  - la **versione attuale**, con il numero di build, così si vede di essere passati a quella nuova;
  - l'interruttore **«Aggiornamento automatico»**, acceso di serie. Se è acceso, Keel controlla **solo all'avvio**: mai mentre resta aperto, mai a intervalli;
  - il pulsante **«Verifica aggiornamenti»**, che controlla subito. C'è anche come comando, «Keel: Cerca aggiornamenti», e nel menu Keel.
- **Se c'è una versione nuova**, sia col controllo automatico sia col pulsante, compare una **finestrella sopra Keel**:
  - il numero della versione nuova e di quella attuale;
  - le novità in poche righe, nella lingua dell'interfaccia;
  - due pulsanti: «Aggiorna e riavvia» e «Più tardi».
- **Con un clic** Keel scarica la versione nuova, ne controlla la firma, la installa e **si riavvia da solo** già aggiornato.
  - Le schede, i terminali e i file non salvati tornano come dopo un normale riavvio.
  - Impostazioni, estensioni e il futuro `style.css` stanno nella cartella dei dati, quindi non vengono toccati.
- **Col pulsante e nessuna versione nuova:** un messaggio breve, «Keel è aggiornato (0.2.0)».
- **Senza rete:**
  - all'avvio il controllo fallisce in silenzio;
  - col pulsante, un messaggio dice che non è stato possibile controllare.
- **Il motore è di Keel,** nel crate `update`, e non Sparkle come pensato all'inizio (decisione dell'autore del 23 novembre). Keel scarica già tutto con il `curl` del Mac. Un motore suo costa poche centinaia di righe di Rust e si prova con i test automatici. Sparkle avrebbe voluto un framework compilato dentro la repo, e da Rust la finestrella di Keel richiede un'interfaccia Objective-C fragile, che i test non raggiungono. Si rinuncia solo agli aggiornamenti «della sola parte cambiata»: si scarica sempre lo `.zip` intero.
- **La firma degli aggiornamenti** è una chiave Ed25519 dell'autore: gratis e diversa da quella di Apple. La sua metà pubblica è `update::PUBLIC_KEY`, vuota finché la fase B non crea la coppia; con la chiave vuota Keel non installa niente.
  - Un pacchetto con una firma sbagliata non si installa.
  - La chiave sta nel portachiavi del Mac dell'autore, con una copia di riserva fuori dal Mac, e mai nella repo.

### Due modi per installarlo la prima volta
- **Script di installazione, il modo principale:**
  ```bash
  curl -fsSL https://keel.mezzalab.eu/install.sh | sh
  ```
  Serve **solo la prima volta**: gli aggiornamenti dopo li fa Keel. Lo script non resta sul Mac e non gira in sottofondo. Keel si apre senza avvisi perché macOS mette l'etichetta «quarantena», che fa comparire l'avviso, solo ai file scaricati dal browser, non a quelli scaricati da `curl`.
- **DMG, la seconda scelta,** per chi non vuole il terminale. La prima volta va aperto da Impostazioni di Sistema → Privacy e sicurezza → «Apri comunque». Dal macOS 15 il tasto destro → Apri non basta più.

### Il sito e i file
- **Il sito:**
  - è KeelSite, su **keel.mezzalab.eu**, ospitato sul **mini PC** come gli altri gestionali;
  - il sottodominio passa dal tunnel Cloudflare `miniPC` che c'è già;
  - ha il **deploy automatico a ogni push su `main`** di KeelSite, con un runner GitHub sul mini PC come negli altri progetti.
- **I pacchetti di Keel non stanno nella repo del sito,** che a ogni versione si appesantirebbe per sempre. Stanno in una cartella sul mini PC fuori dalla repo, per esempio `/srv/keel-site/releases`, servita insieme alle pagine:
  - `https://keel.mezzalab.eu/updates.json`, l'elenco delle versioni letto da Keel;
  - `https://keel.mezzalab.eu/install.sh`;
  - `https://keel.mezzalab.eu/releases/Keel-<versione>.dmg` e `.zip`: il DMG per chi installa a mano, lo `.zip` (fatto con `ditto`) per gli aggiornamenti.
- **Solo due versioni sul mini PC:** l'attuale e la precedente, circa 80 MB in tutto. A ogni rilascio la precedente si cancella, l'attuale diventa precedente e la nuova diventa attuale. La precedente serve:
  - a chi ha la penultima, per scaricare solo la parte cambiata;
  - per tornare indietro in un attimo se la nuova ha un problema.

  Chi è molto indietro scarica l'attuale intera.
- **Il deploy del sito tocca solo le pagine.** I pacchetti li carica lo script di rilascio, attraverso la VPN.

### Altre scelte
- **Niente App Store:** la sua sandbox vieta terminale, language server, debugger e accesso libero alle cartelle.
- **Homebrew** viene dopo, se servirà: quello ufficiale vuole app notarizzate da Apple.
- **Firma di Apple** (Developer ID e notarizzazione, 99 $ l'anno): rimandata. Quando arriverà, il DMG tornerà il modo principale, perché si apre con un doppio clic, e lo script resterà come alternativa.

---

## Come funziona, nei dettagli

### Come Keel capisce di essere indietro
1. **Keel sa la sua versione.** È nel suo `Info.plist`:
   - `CFBundleShortVersionString`, quella che si legge, come `0.2.0`;
   - `CFBundleVersion`, un numero di build che cresce sempre: 1, 2, 3…

   Oggi `scripts/bundle-macos.sh` mette in tutti e due la versione di `Cargo.toml` (0.1.0): il numero di build va separato.
2. **L'elenco `updates.json`** elenca le versioni presenti. Per ognuna:
   - il numero e la build;
   - la data e la versione minima di macOS;
   - le novità in italiano e in inglese;
   - l'indirizzo del pacchetto, il suo peso e la sua firma EdDSA.
3. **All'avvio**, quando la finestra è già sullo schermo e fuori dal thread dell'interfaccia, Keel legge l'elenco delle versioni. Se la build più alta è maggiore della sua, mostra la finestrella. L'avvio non rallenta: è una richiesta sola di pochi KB.

### Il rilascio, ordine dei passi
Lo script di rilascio fa il cambio in quest'ordine, così chi scarica in quel momento non trova mai un file a metà:
1. carica la versione nuova completa;
2. aggiorna l'elenco delle versioni e `install.sh`, con il nuovo indirizzo e la nuova impronta;
3. per ultimo cancella la versione più vecchia, la terza.

### `install.sh`
Circa 40 righe, leggibili all'indirizzo stesso: la pagina di download lo dice. Fa solo questo:
1. controlla Apple Silicon e macOS 13 o successivo; altrimenti si ferma e dice perché;
2. scarica il DMG dell'ultima versione da keel.mezzalab.eu, solo in HTTPS;
3. controlla l'**impronta SHA-256** del file con quella scritta nello script, che lo script di rilascio aggiorna a ogni versione; se non coincidono si ferma senza installare niente;
4. copia `Keel.app` in `/Applications`, o in `~/Applications` se Applicazioni non è scrivibile. Una versione già presente viene sostituita solo quando la nuova è completa;
5. toglie i file temporanei e apre Keel.

Perché è sicuro e non invadente:
- **niente password né `sudo`:** non può toccare il sistema;
- **tocca solo `Keel.app`:** non lascia servizi, processi in sottofondo, programmi in più, né cambi al terminale o al `PATH`;
- **protetto se la connessione cade:** tutto il codice sta dentro una funzione che parte solo all'ultima riga, quindi uno script arrivato a metà non fa niente, come in rustup;
- **usa solo quello che c'è su ogni Mac:** `curl`, `hdiutil`, `shasum`.

---

## Le fasi

### Fase A — Aggiornamenti dentro Keel
1. **Versione e build:**
   - il numero di build in `CFBundleVersion`, separato dalla versione;
   - Keel le legge da sé;
   - «Informazioni su Keel» le mostra.
2. **Il motore, crate `update`:**
   - legge l'elenco con `curl`;
   - sceglie la versione più nuova che gira su quel macOS;
   - scarica lo `.zip` vicino all'app, in `.keel-update`;
   - controlla peso, SHA-256 e firma Ed25519;
   - lo scompatta con `ditto` e controlla che dentro ci sia Keel (`com.manuelmezzanotte.keel`), della build promessa e con la firma del codice valida;
   - dopo la chiusura di Keel, un processo a parte (`keel --finish-update`) aspetta che Keel sia uscito, sposta la versione vecchia, mette al suo posto la nuova e, se qualcosa va storto, rimette la vecchia;
   - infine riapre Keel e lascia un biglietto (`update-outcome.json` nella cartella dei dati) che il Keel nuovo legge per dire com'è andata.
3. **Impostazioni:**
   - la versione attuale;
   - l'interruttore `update.automatic`, acceso di serie;
   - il pulsante «Verifica aggiornamenti»;
   - il comando nella palette e nel menu Keel;
   - i testi in italiano e in inglese.
4. **La finestrella:**
   - versione nuova e attuale, novità, «Aggiorna e riavvia» e «Più tardi»;
   - durante il download un avanzamento nella finestrella stessa;
   - in caso di errore un messaggio chiaro, e Keel resta com'è.
5. **Riavvio:** prima di sostituire l'app Keel salva la sessione, come con Cmd+Q, poi si riapre aggiornato.
6. **Prove automatiche** con un elenco e pacchetti finti in locale (indirizzi `file://`):
   - versione più nuova, uguale o più vecchia;
   - firma sbagliata;
   - nessuna rete;
   - interruttore spento, che non deve fare nessuna richiesta all'avvio;
   - pulsante.
7. **Misure:** memoria e tempo di avvio prima e dopo gli aggiornamenti, con lo stesso metodo del confronto con VS Code e Cursor (memoria dopo 40 secondi, media di tre avvii).

   **Risultato (23 novembre, versione installata del 22 contro quella nuova, stesso README aperto, cartella dei dati vuota, sette avvii ciascuna, alternati nell'ultimo giro):**
   - **prima schermata:** in media 352 ms prima (tolto un primo avvio a freddo da 609 ms) e 353 ms dopo. Nessuna differenza;
   - **memoria dopo 40 secondi:** varia allo stesso modo nelle due versioni, da 59 a 236 MB, a seconda di quanto la finestra viene davvero disegnata. È il difetto della memoria grafica già noto in `HANDOFF.md`, non gli aggiornamenti: un avvio con la versione vecchia ha dato 137 MB e uno con la nuova 59;
   - **il controllo all'avvio:** con `keel.mezzalab.eu` non ancora online fallisce in silenzio, come deve, senza ritardare la finestra.

### Fase B — Script di rilascio e di installazione
1. **`scripts/release.sh`**, un comando solo:
   0. la prima volta crea la coppia di chiavi Ed25519: la privata nel portachiavi del Mac, con una copia di riserva fuori dal Mac; la pubblica in `update::PUBLIC_KEY` (`crates/update/src/lib.rs`);
   1. alza versione e build: `version` nel `Cargo.toml` della radice e `BUILD` in `crates/keel/src/version.rs`;
   2. prende le novità da un file con una sezione `## it` e una `## en`;
   3. costruisce Keel;
   4. crea DMG e `.zip`;
   5. firma l'aggiornamento;
   6. calcola le impronte;
   7. prepara elenco delle versioni e `install.sh`;
   8. carica tutto sul mini PC nell'ordine giusto e lascia solo due versioni.
2. **`install.sh`**, come descritto sopra.
3. **Prova vera su un Mac pulito**, o con un utente macOS nuovo:
   - installazione con lo script e con il DMG;
   - aggiornamento da una versione alla successiva con la finestrella;
   - controllo che l'avviso di macOS non ricompaia dopo un aggiornamento: Keel scarica con `curl`, che non mette l'etichetta di quarantena, quindi non dovrebbe;

   - la prima prova vera del processo che sostituisce l'app, che i test fanno solo su cartelle finte.

   Se l'avviso ricompare, Keel toglie da solo l'etichetta di quarantena al pacchetto già scaricato e verificato.

**Com'è fatta (23 novembre):**
- `scripts/release.sh VERSIONE NOTE [--to DEST]` fa tutto. Senza `--to` pubblica sul mini PC (`minipc:/srv/keel-site/downloads`, la cartella arriva con la fase D), poi fa il commit `chore(release): …` e il tag `vVERSIONE`, senza push. Con `--to CARTELLA` pubblica in una cartella, per provare, e non fa commit.
- Lo strumento `keel-release` (crate `update`) tiene la chiave privata nel **Portachiavi login**, alla voce «Keel release signing key». Firma, scrive `updates.json` e `install.sh`, e alla fine riscarica la versione pubblicata e la controlla come farebbe Keel.
- **La chiave vera esiste dal 23 novembre.** La pubblica è in `update::PUBLIC_KEY`. **Va fatta una copia della privata fuori dal Mac**, per esempio nel gestore delle password: da Accesso Portachiavi → «Keel release signing key» → Mostra password. Senza quella copia, perso il Mac, nessun Keel installato potrebbe più aggiornarsi.
- `scripts/install.sh` è il modello di `install.sh`: il rilascio ci scrive versione, indirizzo e impronta del DMG.

**Provato:**
- test automatici: pubblicazione in una cartella, download e controllo come fa Keel; due versioni tenute e la terza tolta; build non più nuova rifiutata; `install.sh` lanciato davvero su un'immagine disco, che installa, sostituisce e con un'impronta sbagliata non installa niente;
- prova vera con l'app vera: due rilasci (0.1.1 build 2 e 0.1.2 build 3) in una cartella, ognuno riscaricato e controllato; installazione della 0.1.1 con `install.sh` (`keel --version` dà «Keel 0.1.1 (2)», firma del codice valida, nessuna etichetta di quarantena); nella cartella restano solo le due versioni. Dopo la prova i numeri di versione sono tornati a 0.1.0 (1).

**Da provare a mano** (il clic non si può fare da qui): l'aggiornamento dalla finestrella e il processo che sostituisce l'app. I comandi sono nel messaggio della fase B; la prova usa una cartella dei dati sua, quindi i dati veri non vengono toccati.

### Fase C — Pagina di download su KeelSite
1. **Pagina `/download`, e `/en/download` in inglese,** con poche righe di spiegazione: chi scarica Keel se ne intende un po'.
   - In evidenza la riga dello script, con «Copia» e il collegamento per leggerlo.
   - Sotto, «Oppure scarica il DMG», con i passaggi brevi per «Apri comunque». Il testo è onesto e non spaventa: «Keel non è ancora firmato da Apple. Non vuol dire che sia pericoloso, solo che Apple non l'ha controllato.»
   - Versione attuale, data, peso, requisiti (macOS 13 o successivo, Apple Silicon) e impronta SHA-256, letti dall'elenco delle versioni al momento del build del sito.
2. **I pulsanti della pagina principale** portano qui: `downloadUrl` in `src/site.ts` diventa l'indirizzo della pagina.
3. **Le domande del sito:** come si aggiorna Keel e cosa fa lo script.

**Com'è fatta (23 novembre, KeelSite `ae6d32e`):**
- `/download/` e `/en/download/`: titolo, requisiti, poi due schede. «Dal Terminale» (consigliato) ha la riga dello script con «Copia» e «Leggi lo script»; «Con l'immagine disco» ha il pulsante del DMG col peso e i tre passaggi della prima apertura. Sotto, versione, data e SHA-256 del DMG, e una riga su come si aggiorna Keel.
- **I dati non sono nel sito:** la pagina li legge al momento da `/updates.json`, a cui `release.sh` ora aggiunge per ogni versione indirizzo, peso e SHA-256 del DMG (`diskImage`; Keel lo ignora). Così un rilascio non chiede un nuovo build del sito. Senza elenco, come in sviluppo, la pagina dice che la prima versione è in arrivo.
- I pulsanti «Scarica per Mac» della home portano alla pagina (`site.downloads` in `src/site.ts`).
- Due domande nuove: come si aggiorna Keel e cosa fa lo script; quella sulla rete dice anche del controllo all'avvio.
- La pagina carica solo il suo codice: circa 6,5 kB più React, senza il mockup della home.
- **Controllato:** il build (TypeScript, Vite e le quattro pagine disegnate in HTML) e il testo delle due pagine. L'aspetto non l'ho guardato: va visto da te con `pnpm dev` su `/download/`.

### Fase D — Online su keel.mezzalab.eu
1. **Sul mini PC:**
   - la cartella del sito e quella dei pacchetti, `/srv/keel-site`;
   - un piccolo server statico in Docker su una porta locale, con le intestazioni giuste (`updates.json` mai in cache, i pacchetti sì).
2. **Cloudflare:** il sottodominio `keel.mezzalab.eu` sul tunnel `miniPC` verso quella porta.
3. **Deploy automatico:**
   - un runner GitHub sul mini PC, solo per KeelSite;
   - a ogni push su `main`: build, controlli, sostituzione delle pagine in un colpo solo, prova che il sito risponda;
   - la cartella dei pacchetti non viene mai toccata.
   - Come per Silo e la dashboard: cartella `deploy/` con `install.sh` per il mini PC, lanciato una volta dall'autore, che ha la password di `sudo`.
4. **Primo rilascio:** `release.sh` carica la prima versione ufficiale, poi la pagina di download, elenco delle versioni e script rispondono.
5. **Dashboard del mini PC:** la card del sito, come per le altre app.
6. **Documenti:**
   - nel README di Keel, l'installazione e la sezione «Privacy», che deve dire che Keel controlla gli aggiornamenti su keel.mezzalab.eu all'avvio, se l'interruttore è acceso;
   - `HANDOFF.md`;
   - il README di KeelSite.

**Com'è fatta (23 novembre, KeelSite `d15db7b`):**
- **Il container:** nginx senza root (`nginxinc/nginx-unprivileged:1.29-alpine`), file system di sola lettura, `cap_drop: ALL`, su `127.0.0.1:3006`. La cartella `/srv/keel-site/downloads` è montata in sola lettura; la scrive `release.sh` di Keel come utente `server`.
- **Le regole di nginx:** pagine sempre richieste di nuovo, file di build tenuti un anno, `updates.json` mai tenuto, `install.sh` come testo da leggere, i DMG e gli `.zip` di `releases/` tenuti un giorno.
- **Il deploy** ricalca quello della dashboard e di Silo, senza database né backup. Workflow su `main` con il runner `keel-site-production`; `keel-site-deploy` costruisce l'immagine, controlla le quattro pagine in un container senza rete, prova un candidato, scambia e verifica `https://keel.mezzalab.eu/` e `/download/`, altrimenti rimette la versione di prima. Comando di gestione: `sudo keel-site stato|log|riavvia`.
- **Provato sul Mac** con Docker: configurazione di nginx valida, controllo delle pagine come lo fa il deploy, intestazioni e cache giuste, DMG della prova scaricato identico, container che gira come utente `nginx`. I 12 test del deploy passano.

**Passi che restano all'autore** (il mini PC risponde solo con la VPN, e servono `sudo` e Cloudflare):
1. Cloudflare → Networking → Tunnels → `miniPC` → Public hostname → Add: `keel` · `mezzalab.eu` · `HTTP` · `localhost:3006`.
2. Con la VPN accesa, dalla cartella KeelSite: `scp -r deploy compose.production.yaml minipc:keel-site-setup/` e `ssh -t minipc sudo bash keel-site-setup/deploy/install.sh`, col token del runner da GitHub.
3. Push di `develop` e `main` di KeelSite: il workflow pubblica il sito.
4. Primo rilascio di Keel: `scripts/release.sh VERSIONE novita.md` dalla cartella di Keel, con la VPN accesa.

**Online (23 novembre):**
- **Cloudflare:** rotta `keel.mezzalab.eu` → `http://localhost:3006` nel tunnel `miniPC`, con il record DNS creato da Cloudflare. L'interfaccia è cambiata rispetto ai README degli altri progetti: ora è Tunnels → `miniPC` → **Routes** → Add route → **Published application**.
- **Mini PC:** `install.sh` lanciato dall'autore (runner `server-keel-site` attivo); primo deploy da `main` riuscito in 24 secondi.
- **Primo rilascio:** `scripts/release.sh 0.1.0 …`. Il mini PC chiede un codice di verifica a ogni connessione SSH, quindi `release.sh` apre una connessione sola e la riusa (`5eb0a2f`).
- **Controllato da fuori:** pagine 200; `curl -fsSL https://keel.mezzalab.eu/install.sh | sh` installa Keel 0.1.0 (2) con firma del codice valida e senza quarantena; `keel-release check` riscarica e verifica l'aggiornamento come fa Keel; il DMG arriva con il tipo giusto.

**Per ogni versione nuova:** si scrive cosa cambia in `CHANGELOG.md` e `docs/CHANGELOG.it.md` e si fa il commit; poi, con la VPN accesa, dalla cartella di Keel, `scripts/release.sh VERSIONE novita.md` (le novità sono il riassunto del changelog), poi il push di `develop` e del tag quando vuoi. La pagina di download del sito non va toccata; la pagina delle novità (`/changelog/`) sì: in KeelSite la versione va aggiunta in cima a `changelogPage.releases` di `src/copy/it.ts` e `src/copy/en.ts`, e il sito va pubblicato.

Ordine: **A → B → C → D**. Il sito va online solo dopo che tutto è stato confermato dall'autore.

---

## Da decidere più avanti

- ~~**La licenza di Keel**~~: decisa il 25 novembre 2026, in `LICENSE` (in italiano `docs/LICENZA.it.md`). Codice da leggere e basta, app gratis da usare anche per lavoro.
- **La prima versione ufficiale:** `0.1.0`, oppure `1.0.0`.
- **La lingua principale del sito,** oggi l'italiano.

## Rischi

| Rischio | Cosa facciamo |
|---|---|
| Qualcuno teme lo script `curl … \| sh` | È corto, leggibile al suo indirizzo, senza `sudo`, e controlla l'impronta; il DMG resta come alternativa |
| Senza notarizzazione l'avviso di macOS ricompare dopo un aggiornamento | Si prova nella fase B prima di andare online; se succede, Keel toglie l'etichetta di quarantena al pacchetto verificato |
| Si perde la chiave di firma degli aggiornamenti | Copia nel portachiavi e una di riserva fuori dal Mac: senza, gli utenti non riceverebbero più aggiornamenti |
| Un aggiornamento rotto arriva a tutti | La versione si prova sul Mac dell'autore prima del rilascio; la precedente resta sul mini PC e l'elenco delle versioni si può riportare a lei in un attimo |
| Il mini PC è spento o senza rete | Il controllo all'avvio fallisce in silenzio; installazioni e aggiornamenti riprendono quando torna |
| macOS cambia le regole per le app non firmate | È il motivo per passare poi alla firma di Apple; tutto il resto non cambia |
