# Keel — i dettagli

I piccoli dettagli che rendono comodo un IDE: quelli a cui non pensi finché mancano. Raccoglie cosa fanno VS Code, Cursor, Zed, JetBrains, Xcode, Sublime Text, i programmi per git e il Mac stesso, confrontato con quello che Keel fa oggi. Alla fine c'è l'ordine in cui farli, deciso da me.

*Scritto il 19 novembre 2026, sul commit `12db1b4` di `develop`. Aggiornato il 20 novembre 2026: i nove giri sono fatti, dal commit `db6bbbe` al `66e6bfb`, su `develop`. La colonna «Keel oggi» dice ora com'è Keel dopo i giri. Aggiornato di nuovo il 22 novembre 2026 con i lavori dal commit `50e24d1` al `2e4b0a5`, su `develop` e su GitHub: quasi tutto quello che era rimasto, più le funzioni grandi raccontate in «Le funzioni nuove».*

---

## Come è fatto questo documento

- **Ricerca.** Ho letto le pagine ufficiali di VS Code (la 1.147 è uscita ieri), di Zed e di Cursor. Quello che riguarda JetBrains, Xcode, Sublime, GitHub Desktop e macOS viene da quello che so di questi programmi, non da pagine lette oggi. I link sono in fondo.
- **Confronto con Keel.** Per ogni punto ho guardato nel codice se c'è già. Nella colonna «Keel oggi»:
  - **rotto** vuol dire che c'è ma si comporta male;
  - **a metà** vuol dire che ne manca un pezzo;
  - **manca** vuol dire che non c'è.

  Le cose che Keel fa già bene non sono elencate. In tutto erano 143 punti: 6 rotti, 13 a metà, 124 che mancavano. Oggi: 131 fatti, 9 a metà, 3 non ancora (vedi «Cosa non c'è ancora»).
- **Giro.** Il numero dice in quale giro di lavoro lo farei. I giri sono descritti alla fine, in «L'ordine dei lavori».
- **Fuori da qui.** Le funzioni grandi di cui abbiamo parlato (chat in parallelo, Linear, Docker) hanno il loro spazio. Il commit scritto da Claude e le pull request ora ci sono: sono in «Le funzioni nuove».

---

## Come provarlo

Keel nuovo è installato in /Applications: chiudi quello aperto e riaprilo. Ecco cosa guardare, giro per giro.

- **Giri 1–3, Git e differenze.**
  - Clicca un file nella vista Git, poi torna ai file: l'albero è su quel file. Scorri giù, cambia vista, torna: sei dov'eri.
  - Fai un push: un avviso dice quanti commit sono partiti; se fallisce, una finestra dice perché e cosa fare.
  - Apri una differenza: due colonne uguali, parole cambiate evidenziate, F7 per passare da un blocco all'altro, Stage e Annulla per blocco.
- **Giri 4–6, navigare e scrivere.** Ctrl+- per tornare indietro, Ctrl+Tab fra i file recenti, Alt+F12 per sbirciare una definizione, Cmd+C senza selezione per copiare la riga, parentesi colorate.
- **Giro 7, terminale.**
  - Apri un terminale ed esegui qualche comando: accanto a ognuno c'è un pallino, blu o rosso. Un clic sul pallino apre il suo menu.
  - Cmd+↑ e Cmd+↓ saltano fra i comandi; scorrendo dentro un output lungo, il comando resta in cima.
  - Un comando che fallisce mostra una barra in basso a destra: «Libera la porta 3000» se la porta è occupata, e «Chiedi a Claude».
  - Ctrl+Alt+R esegue un comando recente; Cmd+L passa a Claude la selezione o l'ultimo comando.
  - Chiudi e riapri Keel: i terminali tornano con il loro testo.
- **Giro 8, il Mac.**
  - Menu Finestra (Cmd+M contrae) e Aiuto con la ricerca; tasto destro sull'icona nel Dock per i progetti recenti.
  - Spazio su un'immagine nell'albero: Quick Look. Trascina un file dall'albero o una scheda fuori dalla finestra: va nel Finder.
  - Nella barra di stato clicca riga e colonna, «Spazi: 2», «UTF-8», «LF» e il linguaggio. Premi Cmd+K: la barra dice che aspetta il secondo tasto.
  - Con Keel in secondo piano, Claude che finisce o chiede un permesso mette un numero sull'icona nel Dock.
- **Giro 9, il resto.**
  - F3 mette un segnalibro, Cmd+F3 li elenca, Cmd+Alt+L e Cmd+Alt+J ci vanno.
  - Cmd+Alt+V incolla dalla cronologia degli appunti. Un'immagine incollata in un file Markdown viene salvata accanto al file.
  - Sposta una funzione in un file e guarda la differenza: è in blu, «spostato alla riga…». Cambia un'immagine del progetto e apri la sua differenza.
  - I file cambiati da Claude hanno una scintilla nell'albero finché non li apri. Nei Problemi, «Correggi con Claude» compare passando sopra.
  - Il cursore scivola mentre scrivi; in Impostazioni → Aspetto ci sono il raggio e lo spazio dei pannelli e le schede su più righe.

### Le funzioni nuove (21–22 novembre)

**Schede trascinate.** Prendi una scheda e portala su un bordo dell'editor: destra, sinistra, sopra o sotto, e l'editor si divide lì; al centro la scheda passa in quel gruppo. Il punto in cui finirà si colora mentre trascini. Da tastiera: Cmd+\ divide a destra, Cmd+K Cmd+\ divide in basso, Cmd+K e poi Cmd con una freccia passa al gruppo da quella parte.
- *Limiti:* i gruppi uno sopra l'altro hanno tutti la stessa altezza, e i livelli sono due (colonne, e gruppi dentro una colonna).

**Claude nell'editor.** Cmd+I con qualche riga selezionata: scrivi cosa cambiare, Keel mostra le differenze e tu accetti o rifiuti. In Controllo versione la scintilla nel campo del messaggio lo fa scrivere a Claude.

**Test.** Accanto a ogni test c'è un triangolo: un clic lo esegue, il tasto destro propone anche il debug, il doppio triangolo esegue il gruppo. La vista Test li elenca tutti con l'esito. Cmd+; e poi C esegue il test sotto il cursore.
- *Limiti:* l'esito arriva dal terminale, quindi serve zsh con l'integrazione della shell, che è accesa di serie.

**Git, dentro l'editor.** Fermati sull'autore in fondo alla riga: compaiono il messaggio intero, «Apri il commit» e «Apri su GitHub». In una differenza non in stage, doppio clic su una riga a destra per modificarla: Cmd+Invio la scrive nel file, Esc lascia tutto com'era. Se il file nel frattempo è cambiato in quel punto, Keel lo dice e non scrive.

**Cerca.** Il pulsante in cima ai risultati (o «Cerca: Apri i risultati in una scheda») apre i risultati con il loro codice: si modificano lì, e Cmd+S li scrive nei file. F12 va alla riga nel suo file. Un pezzo che nel file è cambiato nel frattempo non viene scritto, e resta segnato come non salvato.

**Righelli, riferimenti e tipi.** In `settings.json`, `"editor.rulers": [80, 100]`. Accanto alle funzioni c'è «3 riferimenti», cliccabile. Tieni premuto Ctrl+Opzione per vedere i tipi; `editor.inlayHints.enabled` li tiene sempre accesi.

**Attività.** Cmd+Maiusc+B compila; «Attività: Esegui» nella palette elenca quelle di `.vscode/tasks.json` e quelle trovate da Keel (cargo, npm, TypeScript). Gli errori finiscono nei Problemi, alla loro riga.
- *Limiti:* un'attività non parte mai da sola all'apertura di una cartella, per sicurezza; le attività definite dalle estensioni e le espressioni regolari con lookahead non ci sono.

**Debugger.** Cmd+Maiusc+D apre la vista. Un clic a sinistra dei numeri di riga (o F9) mette un punto d'interruzione, il tasto destro ci mette una condizione o un messaggio. F5 avvia e continua, F10 passa oltre, F11 entra, Maiusc+F11 esce, Maiusc+F5 ferma. Le variabili, lo stack e le espressioni osservate sono nella vista; i valori compaiono anche in fondo alle righe; la Debug Console (Cmd+Maiusc+Y) valuta espressioni. Le configurazioni sono quelle di `.vscode/launch.json`; senza, Keel ne propone di sue.
- *Provato davvero:* un programma Rust con lldb-dap (quello di Xcode) e uno Python con debugpy (quello dell'estensione Python di VS Code).
- *Non ancora provato:* Node e TypeScript. Vogliono `js-debug` di Microsoft, che su questo Mac non c'è: Keel lo scarica da GitHub (1,2 MB) solo se glielo chiedi, e ne controlla l'impronta.
- *Limiti:* niente valore passando sopra una variabile nel codice; niente punti d'interruzione su funzioni o su dati; un sorgente che non sta sul disco non si apre; LLDB non sa chiamare i metodi di Rust nelle espressioni.

**Pull request.** Clic su «PR #12» nella barra di stato, o «GitHub: Pull request» nella palette. La pagina le elenca (aperte, tue, da rivedere, chiuse); dentro ci sono la descrizione, la conversazione, i controlli e i file. Nelle differenze, doppio clic su una riga per commentarla. Da lì si approva, si chiedono modifiche, si unisce, si passa al ramo. Vuole `gh`, già entrato nel tuo account.
- *Provato davvero:* la lettura, su un repository pubblico. *Non ancora provate su GitHub:* le azioni che scrivono (commenti, approvazioni, unione, creazione), provate solo con un `gh` finto.
- *Limiti:* i commenti sulle righe tolte si leggono solo nella conversazione; una discussione non si può segnare come risolta; le immagini nei testi sono link.

## Cosa non c'è ancora, e perché

- **Fatti con la 0.1.3:** il messaggio del commit in una finestra grande (Maiusc+Esc, o ⤢ sotto il messaggio); Cmd+P con → (nel campo vuoto) o ⌥↩ che apre il file dietro l'elenco e lo lascia aperto; «Non mostrare più» sugli avvisi che si ripetono, con «Avvisi: mostra di nuovo quelli nascosti» per farli tornare.
- **Ancora da fare:**
  - una scheda trascinata fuori che apre una finestra nuova: le finestre di Keel sono una per progetto, e due finestre sullo stesso progetto vorrebbero documenti in comune. Per ora la scheda trascinata fuori porta il suo file nel Finder. Dentro la finestra invece si trascina su un bordo e divide l'editor.
- **A metà:**
  - i suggerimenti in riga: ci sono i tipi, in fondo alla riga perché l'editor di gpui-component non sa disegnare in mezzo al testo; i nomi dei parametri in fondo alla riga non direbbero a quale argomento si riferiscono, quindi non ci sono;
  - l'albero che segue Git Graph, la ricerca, i Problemi e i link del terminale;
  - «Commit e push» come scelta predefinita del pulsante;
  - i simboli nel percorso sopra l'editor;
  - gli spazi visibili solo ai bordi o nella selezione;
  - i risultati della ricerca nella barra di scorrimento;
  - lo scorrimento morbido, che resta quello di macOS (il cursore invece scivola);
  - il numero dei cursori nella barra di stato.

## Le sei cose che hai chiesto tu

Le ho guardate prima di tutto nel codice, per capire da dove viene ogni problema.

### 1. Dalla vista Git all'Esplora risorse, sullo stesso file

- **Cosa succede.** Clicchi i18n.rs nella vista Git e si apre «i18n.rs (modifiche)». Torni all'Esplora risorse e l'albero è ancora sul file di prima.
- **Perché.** L'albero segue la scheda attiva solo se è un file sul disco (`reveal_active_in_tree` usa `disk_path`). Una scheda delle differenze non lo è, quindi l'albero non si muove. Lo stesso succede con le differenze di Git Graph e con le modifiche di Claude.
- **Come lo sistemo:**
  - le schede delle differenze, di un commit e di Claude dicono a quale file appartengono, e l'albero va su quel file;
  - anche un clic nella ricerca, nei Problemi e nel terminale (Cmd+clic su `file:riga`) porta l'albero sul file;
  - l'albero scorre fino al file, centrato, ma solo se è fuori vista: se lo vedi già, non salta.

### 2. L'Esplora risorse ricorda dove eri

- **Cosa succede.** Scorri giù fra 300 file, passi alla ricerca, torni ai file: l'albero è di nuovo in cima.
- **Perché.** La lista dell'albero non ha un segnaposto dello scorrimento che resta tra un disegno e l'altro (`uniform_list` senza `UniformListScrollHandle`). Quando la vista torna, la lista nasce nuova e riparte da zero.
- **Come lo sistemo:**
  - ogni vista della barra laterale tiene il suo scorrimento: file, ricerca, Git, estensioni;
  - lo scorrimento dell'albero si salva con la sessione del progetto, insieme alle cartelle aperte, che già si salvano;
  - riaprendo il progetto ritrovi l'albero come l'avevi lasciato.

### 3. La vista delle differenze, che oggi ha dei difetti

Ho riguardato un'immagine della vista delle differenze, disegnata ieri da `scripts/snapshots.sh`:
- **la colonna di destra è tagliata:** quella di sinistra prende quasi tutto lo spazio, e le righe nuove si leggono a metà;
- **le intestazioni dei blocchi sono quelle grezze di git** (`@@ -601,6 +601,12 @@ pub struct Strings {`), invece di «riga 601 · pub struct Strings» in grigio;
- **le righe nascoste non si aprono:** VS Code e GitHub mostrano «23 righe nascoste» con le frecce per vederne 20 in più o tutte;
- **dentro una riga cambiata non si vede cosa è cambiato:** manca l'evidenziazione delle parole, che Zed ha di serie;
- **non si può scrivere nella colonna di destra**, mentre in VS Code e in Zed sì;
- **mancano le frecce «modifica successiva» e «precedente»** (F7 e Maiusc+F7) e i segni dei cambiamenti nella barra di scorrimento;
- **mancano i pulsanti «metti in stage» e «annulla» per ogni blocco**, accanto ai numeri di riga, come in VS Code dalla 1.88;
- **la scheda ha l'icona generica** al posto di quella del linguaggio.

**Come lo sistemo.** Oggi la vista è il componente di gpui-component, che è di sola lettura e non si può correggere dall'esterno. La rifaccio con l'editor di Keel: due editor affiancati che scorrono insieme, divisi a metà e con il divisorio trascinabile, oppure uno solo con le righe tolte in rosso. Il giro 2 è tutto per lei.

### 4. Il salvataggio automatico come in Cursor e VS Code

- **Cosa c'è oggi.** `files.autoSave` c'è, con i quattro modi di VS Code, ma:
  - è spento di serie, anche se in Cursor lo tieni acceso (l'importazione da Cursor lo aveva trovato tra le tue impostazioni);
  - non si accende dal menu File, come si fa in VS Code e in Cursor («File → Salvataggio automatico» con la spunta);
  - «Cambiando editor» salva solo passando da un editor all'altro, non quando vai nel terminale, nella barra laterale o nel pannello di Claude. Quindi `pnpm dev` nel terminale non vede l'ultima modifica.
- **Come lo sistemo:**
  - acceso di serie, «dopo una pausa» di un secondo, e spegnibile dal menu File con la spunta;
  - salva anche quando il cursore lascia l'editor per qualunque altra parte di Keel, e quando Keel va in secondo piano;
  - salva prima di avviare uno script npm o un'attività, così parte sempre con l'ultima versione;
  - nella scheda il pallino delle modifiche sparisce con una dissolvenza, invece di lampeggiare a ogni salvataggio;
  - resta com'è oggi: non formatta (come VS Code) e non salva da solo un file cambiato anche sul disco.

### 5. Messaggi chiari dopo push, pull e commit

- **Cosa c'è oggi.** Dopo un push riuscito non succede niente (`remote_command` passa una funzione vuota). Un errore diventa una riga nella barra di stato e nell'Output, che si perde facilmente.
- **Come lo sistemo:**
  - **un avviso in basso a destra** che entra con un movimento morbido e sparisce dopo qualche secondo:
    - «Push di 3 commit su origin/develop», con il pulsante «Apri su GitHub»;
    - dopo un pull, «Arrivati 2 commit, 5 file cambiati», con «Vedi i commit» che apre Git Graph (come fa JetBrains);
    - dopo un commit, «Commit fatto», con «Annulla» per qualche secondo (come GitHub Desktop);
  - **una finestra per gli errori,** con il messaggio di git leggibile e l'azione che risolve:
    - push rifiutato perché il remoto è più avanti: «Fai pull e riprova»;
    - branch senza remoto: «Pubblica il branch»;
    - conflitti dopo un pull: «Apri i conflitti»;
    - sempre «Copia l'errore» e «Mostra il registro di git»;
  - **l'avanzamento** nella barra di stato, con il cerchio che gira e il nome dell'operazione («Push in corso…»), e il pulsante del commit che diventa un segno di spunta per un attimo;
  - **la campanella** nella barra di stato, con gli ultimi avvisi da rileggere e «Non disturbare»;
  - **una notifica di macOS** se Keel è in secondo piano quando finisce un push lungo o quando Claude ha finito.

### 6. File toccati colorati nell'albero, con le loro cartelle

- **Cosa c'è oggi.** L'albero colora solo gli errori (rosso) e gli avvisi (giallo), e attenua i file ignorati. Lo stato di git non si vede.
- **Come lo sistemo, come VS Code:**
  - **modificato:** nome giallo-arancio e una «M» a destra;
  - **nuovo, non ancora in git:** verde con «U»;
  - **aggiunto in stage:** verde con «A»;
  - **eliminato:** rosso e barrato, con «D», finché non fai il commit;
  - **rinominato:** «R»;
  - **in conflitto:** viola con «C», più forte di tutti;
  - **le cartelle** che contengono qualcosa di cambiato prendono il colore più importante e un pallino, fino alla cartella del progetto. Anche una cartella chiusa ti dice che dentro c'è qualcosa;
  - **gli errori vincono sul colore** (rosso), ma la lettera di git resta;
  - **gli stessi colori** sulle schede, nell'elenco degli editor aperti e nei risultati della ricerca;
  - si spengono con `explorer.decorations.colors` e `explorer.decorations.badges`, come in VS Code.

---

## Tutto il resto, per area

### Esplora risorse

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Lo scorrimento resta quando cambi vista e quando riapri il progetto | VS Code, Zed | fatto | 1 |
| Il file rivelato si vede: l'albero scorre fino a lui, centrato, solo se è fuori vista | VS Code | fatto | 1 |
| Segue le differenze, Git Graph, la ricerca, i Problemi e i link del terminale | VS Code | a metà: segue le differenze e le modifiche di Claude; Git Graph, ricerca, Problemi e link del terminale non ancora | 1 |
| Colori e lettere di git, propagati alle cartelle con un pallino | VS Code, Zed, JetBrains | fatto | 1 |
| Errori e avvisi propagati alle cartelle | VS Code | fatto | 1 |
| Selezione di più file con Cmd e Maiusc, per spostarli, eliminarli o copiarli insieme | VS Code | fatto | 5 |
| Copia, taglia, incolla e duplica (Cmd+C, Cmd+X, Cmd+V); «nome copia.ts» se il nome c'è già | VS Code | fatto | 5 |
| Cmd+Z annulla l'ultima rinomina, spostamento o eliminazione | VS Code | fatto | 5 |
| Scrivi per cercare nell'albero (Cmd+Alt+F): evidenzia o filtra, e le cartelle dicono quanti ne contengono | VS Code | fatto | 5 |
| Le cartelle che contengono la riga in cima restano appiccicate in alto mentre scorri | VS Code, Zed | fatto | 5 |
| Guide di rientro, più chiare al passaggio del mouse | VS Code, Zed | fatto | 5 |
| Cartelle compatte: `src/main/java` su una riga sola, se ognuna contiene solo la successiva | VS Code, Zed | fatto | 5 |
| File annidati: `package-lock.json` sotto `package.json`, `x.test.ts` e `x.css` sotto `x.ts` | VS Code | fatto | 5 |
| Alt+clic apre di lato; Cmd+Invio apre a destra | VS Code | fatto | 5 |
| Tasto destro: «Cerca nella cartella…», «Apri nel terminale», «Confronta con il selezionato» | VS Code | fatto | 5 |
| Trascinare dal Finder copia i file nel progetto | VS Code | fatto | 5 |
| Trascinare un file nel terminale scrive il suo percorso; nella chat di Claude lo allega | VS Code, Cursor | fatto | 5 |
| Tenendo un file trascinato sopra una cartella chiusa, la cartella si apre | Finder, VS Code | fatto | 5 |
| Trascinare un file dall'albero al Finder, a Mail o a Slack | macOS | fatto | 8 |
| Spazio su un'immagine, un PDF o un video: anteprima Quick Look, come nel Finder | macOS | fatto | 8 |

### Schede e gruppi

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Colori di git e degli errori anche sul nome della scheda | VS Code | fatto | 1 |
| La scheda delle differenze con l'icona del suo linguaggio | — | fatto | 2 |
| Due file con lo stesso nome: la scheda aggiunge la cartella, `index.ts · components` | VS Code | fatto | 4 |
| Aprendo un file la barra delle schede scorre fino a lui, se era nascosto | VS Code | fatto | 4 |
| Clic con il tasto centrale chiude; doppio clic sullo spazio vuoto crea un file | VS Code | fatto | 4 |
| «Chiudi i salvati» (Cmd+K U) | VS Code | fatto | 4 |
| Ctrl+1…9 va alla scheda numero 1…9 | VS Code, Safari | fatto | 4 |
| Schede su più righe quando non ci stanno, a scelta (`workbench.editor.wrapTabs`) | VS Code | fatto | 9 |
| Trascinare una scheda fuori dalla finestra apre una finestra nuova | VS Code | non ancora: trascinata fuori, la scheda porta il suo file nel Finder o in Mail; una finestra nuova per un file dello stesso progetto vuole finestre che condividano i documenti | 8 |

### Salvataggio e file

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Salvataggio automatico acceso di serie, con la spunta nel menu File | VS Code, Cursor | fatto | 1 |
| «Cambiando editor» salva anche andando nel terminale, nella barra o in Claude | VS Code, Zed | fatto | 1 |
| Salva prima di avviare uno script npm o un'attività | — | fatto | 1 |
| Menu File completo: Salva con nome, Salva tutto (Cmd+Alt+S), Ripristina il file, Chiudi la cartella | VS Code | fatto | 1 |
| Cronologia locale: ogni salvataggio conservato, con una linea del tempo per file da cui tornare indietro, anche per un file cancellato | VS Code, JetBrains | fatto | 3 |
| Confronta il file con gli appunti (Cmd+K C) | VS Code | fatto | 3 |

### Git: la vista e il commit

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Avvisi dopo push, pull, fetch, commit, stash e cambio di branch | JetBrains, Tower | fatto | 1 |
| Finestra d'errore con il messaggio di git e l'azione che risolve | VS Code, JetBrains | fatto | 1 |
| «Annulla l'ultimo commit» subito dopo il commit, e dal menu | GitHub Desktop, Zed, VS Code | fatto | 3 |
| Campo del commit: ↑ e ↓ ripescano i messaggi precedenti | VS Code | fatto | 3 |
| Campo del commit: avviso oltre i 50 caratteri nel titolo e i 72 nel corpo, con il segno dei 72 | VS Code, Zed | fatto | 3 |
| Niente in stage: chiede «Metto in stage tutto e faccio il commit?», con «Sempre» | VS Code | fatto | 3 |
| Commit su `main` o `master`: chiede conferma | VS Code | fatto | 3 |
| Commit e push in un gesto, scelto come azione predefinita del pulsante | VS Code, JetBrains | a metà: «Commit e push» e «Commit e sincronizza» nel menu del pulsante, non come scelta predefinita | 3 |
| Prima del push, l'elenco dei commit che stanno per partire | JetBrains | fatto | 3 |
| Fetch da solo ogni 3 minuti, «ultimo fetch 2 minuti fa», e il numero dei commit in arrivo | VS Code, GitHub Desktop | fatto | 3 |
| Cambio di branch con modifiche non salvate in un commit: «Portale con te» o «Lasciale qui» (stash) | GitHub Desktop | fatto | 3 |
| Elenco degli stash, con Applica, Estrai, Elimina e le differenze | VS Code, Zed | fatto | 3 |
| Branch nuovo senza remoto: «Pubblica il branch» al posto di Push, poi «Crea pull request» | VS Code, GitHub Desktop | fatto | 3 |
| Vista ad albero o a elenco dei file cambiati | VS Code, Zed | fatto | 3 |
| Commit con il campo più grande in una scheda (Maiusc+Esc), per i messaggi lunghi | Zed, VS Code | fatto con la 0.1.3, in una finestra grande | 3 |

### Git: dentro l'editor

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Chi ha cambiato la riga del cursore, in grigio a fine riga: «Manuel, 3 giorni fa · fix: …» | VS Code, Zed | fatto | 3 |
| Passando sopra: il messaggio intero, l'autore, «Apri il commit» e «Apri su GitHub» | VS Code, JetBrains | fatto: la scheda compare fermandosi sull'autore in fondo alla riga | 3 |
| Clic sulla barra colorata a sinistra: la differenza dentro l'editor, con Stage, Annulla, Successiva, Precedente | VS Code | fatto | 2 |
| «Copia il link su GitHub» della riga o della selezione | Zed | fatto | 3 |
| Storia delle righe selezionate (`git log -L`) | JetBrains | fatto | 3 |

### Git: le differenze

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Colonne a metà, con il divisorio trascinabile | VS Code | fatto | 2 |
| Intestazioni dei blocchi leggibili, con la funzione in cui si trovano | GitHub | fatto | 2 |
| Righe nascoste da aprire: 20 in su, 20 in giù o tutte | VS Code, GitHub | fatto | 2 |
| Parole cambiate evidenziate dentro la riga | Zed, VS Code | fatto | 2 |
| Colonna di destra modificabile, con il salvataggio | VS Code, Zed | fatto: doppio clic su una riga, o la matita del blocco, apre le righe in un riquadro; Cmd+Invio le scrive nel file | 2 |
| Stage e Annulla per ogni blocco, accanto ai numeri di riga | VS Code | fatto | 2 |
| F7 e Maiusc+F7 per passare da una modifica all'altra; i segni nella barra di scorrimento | VS Code | fatto | 2 |
| In linea da sola quando la finestra è stretta (sotto i 900 pixel) | VS Code | fatto | 2 |
| «Ignora gli spazi», per vedere solo i cambiamenti veri | VS Code, GitHub Desktop | fatto | 2 |
| In cima: nome del file, «+12 −3» e «Apri il file» alla stessa riga | VS Code, GitHub | fatto | 2 |
| Tutte le modifiche in una sola pagina che scorre, file dopo file, modificabile | Zed, VS Code | fatto: modificabile allo stesso modo | 2 |
| Immagini cambiate: prima e dopo affiancate, o una sopra l'altra con un cursore | GitHub Desktop | fatto | 9 |
| Codice spostato riconosciuto come spostato, non come tolto e aggiunto | VS Code | fatto | 9 |

### Navigare

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Indietro e avanti tra i punti visitati (Ctrl+- e Ctrl+Maiusc+-), anche con i tasti laterali del mouse | VS Code | fatto | 4 |
| Torna all'ultima modifica (Cmd+K Cmd+Q) | VS Code, JetBrains | fatto | 4 |
| Ctrl+Tab: elenco dei file recenti, tenendo Ctrl si scorre | VS Code | fatto | 4 |
| Luoghi recenti: i punti dove sei stato, con un pezzetto di codice ciascuno | JetBrains | fatto | 4 |
| Cmd+P: scorrendo l'elenco il file si vede già nell'editor, Esc torna dov'eri | Sublime, VS Code | fatto | 4 |
| Cmd+P: `file.ts:42` apre alla riga 42; `@` i simboli del file, `#` quelli del progetto, `>` i comandi | VS Code, Sublime | fatto | 4 |
| Cmd+P: freccia a destra apre il file in secondo piano e lascia aperto l'elenco | VS Code | fatto con la 0.1.3 (anche ⌥↩) | 4 |
| Sbirciare la definizione e i riferimenti in un riquadro dentro l'editor (Alt+F12, Maiusc+F12) | VS Code | fatto | 4 |
| Gerarchia delle chiamate: chi chiama questa funzione, e chi chiama quelli | VS Code, JetBrains | fatto | 4 |
| Struttura del file in una vista della barra laterale, che segue il cursore | VS Code, Zed | fatto | 4 |
| Breadcrumb: un clic su un pezzo apre l'elenco dei fratelli, file o simboli | VS Code, Xcode | a metà: una cartella elenca cosa contiene; i simboli no | 4 |
| Vai alla parentesi corrispondente (Cmd+Maiusc+\\) | VS Code | fatto | 6 |
| Segnalibri sulle righe, con l'elenco per tornarci | JetBrains, Xcode | fatto | 9 |

### Scrivere

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Rinominando un tag HTML o JSX si rinomina anche quello di chiusura | VS Code | fatto | 6 |
| Parentesi colorate per livello, con la guida della coppia attiva | VS Code, Zed | fatto | 6 |
| Selezioni del testo e scrivi `"` o `(`: il testo finisce dentro | VS Code, Sublime | fatto | 6 |
| Spazi visibili nella selezione o ai bordi (`editor.renderWhitespace`) | VS Code, Zed | a metà: `editor.renderWhitespace` con «all» | 6 |
| Righelli verticali a 80 o 100 colonne (`editor.rulers`) | VS Code | fatto | 6 |
| Suggerimenti in riga: tipi e nomi dei parametri, attenuati, tenendo premuto un tasto (`editor.inlayHints`) | VS Code, Zed | a metà: i tipi, in fondo alla riga e non in mezzo, tenendo premuto Ctrl+Opzione; i nomi dei parametri no | 6 |
| Quadratino del colore accanto a `#2F74F5` o `rgb(…)`, con il selettore | VS Code | fatto | 6 |
| «3 riferimenti» sopra le funzioni, cliccabile (CodeLens) | VS Code | fatto: in fondo alla riga, non sopra | 6 |
| Le altre occorrenze del simbolo evidenziate, e segnate nella barra di scorrimento | VS Code, Zed | fatto | 6 |
| Errori, risultati della ricerca e modifiche segnati nella barra di scorrimento | VS Code, Zed | a metà: errori, avvisi, modifiche, occorrenze e segnalibri; non i risultati della ricerca | 6 |
| Cmd+C senza selezione copia la riga intera; Cmd+X la taglia | VS Code, Sublime | fatto | 6 |
| Ordina le righe, togli i doppioni, unisci le righe, maiuscole e minuscole, snake e camel case | VS Code, Sublime | fatto | 6 |
| Espandi e restringi la selezione per blocchi (Ctrl+Maiusc+Cmd+→ e ←) | VS Code, JetBrains | fatto | 6 |
| Spostando un file, gli import che lo usano si aggiornano da soli (TypeScript) | VS Code | fatto | 5 |
| Cursore che scivola invece di saltare, e scorrimento morbido | VS Code, Zed | a metà: il cursore scivola; lo scorrimento è quello di macOS | 9 |
| Incollare un'immagine in un Markdown la salva nel progetto e scrive il link | VS Code | fatto | 9 |
| Appunti con la cronologia: Cmd+Maiusc+V incolla una delle ultime cose copiate | JetBrains | fatto, con Cmd+Alt+V: Cmd+Maiusc+V resta l'anteprima del Markdown, come in VS Code | 9 |

### Cerca

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| I risultati si aprono in una scheda che puoi tenere e modificare, come in Zed | Zed, VS Code | fatto | 4 |
| «Cerca nella cartella…» dall'albero riempie «file da includere» | VS Code | fatto | 5 |
| Cerca solo nei file aperti | VS Code | fatto | 4 |
| Togli un risultato o un file dall'elenco con la ×, prima di sostituire | VS Code | fatto | 4 |
| Il testo selezionato o la parola sotto il cursore riempie già il campo di ricerca | VS Code | fatto | 4 |

### Terminale

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Un pallino accanto a ogni comando: blu se è andato bene, rosso se è fallito (integrazione con la shell) | VS Code | fatto | 7 |
| Cmd+↑ e Cmd+↓ saltano da un comando all'altro | VS Code, iTerm | fatto | 7 |
| Il comando in cima resta appiccicato mentre scorri il suo output | VS Code | fatto | 7 |
| «Copia l'output dell'ultimo comando» e «Esegui un comando recente» | VS Code | fatto | 7 |
| Correzioni rapide: porta occupata → «Chiudi il processo»; push senza upstream → «Imposta upstream» | VS Code | fatto | 7 |
| Nome della scheda preso dal programma che gira (node, cargo, claude) | VS Code, iTerm | fatto | 7 |
| Barra di avanzamento nella scheda, per i programmi che la mandano | VS Code, Ghostty | fatto | 7 |
| Notifica quando un comando lungo finisce e Keel è in secondo piano | iTerm, Ghostty | fatto | 7 |
| Riaprendo Keel, i terminali tornano con il loro testo | VS Code | fatto | 7 |
| Selezione o comando fallito → «Chiedi a Claude» | Cursor | fatto | 7 |

### Avvisi e avanzamento

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Avvisi in basso a destra, che entrano e escono con un movimento, impilati se sono più di uno | VS Code, Zed | fatto | 1 |
| Ogni avviso con le sue azioni, e «Non mostrare più» per quelli che si ripetono | VS Code, JetBrains | fatto (il secondo con la 0.1.3) | 1 |
| Campanella con gli avvisi passati e «Non disturbare» | VS Code | fatto | 1 |
| Avanzamento nella barra di stato per git, i language server e le installazioni | VS Code | fatto | 1 |
| Notifiche di macOS solo quando Keel è in secondo piano | macOS | fatto | 1 |

### Barra di stato

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Clic su riga e colonna → vai alla riga | VS Code, Zed | fatto | 8 |
| Clic sul rientro → spazi o tab, 2 o 4, e converti il file | VS Code | fatto | 8 |
| Clic sulla codifica → riapri o salva con un'altra codifica; oggi è sempre scritto «UTF-8» | VS Code | fatto | 8 |
| LF o CRLF, con il clic per cambiarlo | VS Code, Zed | fatto | 8 |
| Clic sul linguaggio → cambialo per questo file | VS Code, Zed | fatto | 8 |
| Quanti caratteri e righe sono selezionati, e quanti cursori ci sono | VS Code | a metà: i caratteri selezionati sì, il numero dei cursori no | 8 |
| Dopo Cmd+K: «Cmd+K premuto, aspetto il secondo tasto» | VS Code, Zed | fatto | 8 |
| Branch con ↓2 ↑3: un clic sincronizza, il cerchio gira mentre lavora | VS Code | fatto | 3 |

### Claude, i dettagli piccoli

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| I file creati o cambiati da Claude segnati nell'albero finché non li guardi | Cursor | fatto | 9 |
| Barra di revisione in basso nell'editor: «file 1 di 3», Tieni e Annulla per ogni blocco e per tutto | Cursor | fatto | 9 |
| Trascinare un file o una cartella nella chat la allega; incollare un'immagine la allega | Cursor | fatto | 9 |
| Problemi e test falliti: «Correggi con Claude» | Cursor | fatto | 9 |
| Notifica di macOS e numero sul Dock quando Claude ha finito o aspetta un permesso | Cursor | fatto | 8 |

### Il Mac per davvero

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Icona del file nel titolo: Cmd+clic mostra il percorso, trascinarla trascina il file | macOS, Xcode | fatto: Cmd+clic o tasto destro sul nome del progetto, in alto a sinistra; trascinare un file fuori si fa dall'albero o dalla scheda | 8 |
| Pallino nel pulsante rosso della finestra quando ci sono modifiche non salvate | macOS | fatto | 8 |
| Menu Finestra: Contrai (Cmd+M), Ingrandisci, l'elenco delle finestre aperte | macOS | fatto | 8 |
| Menu Aiuto con la ricerca dei comandi, come in ogni app del Mac | macOS | fatto | 8 |
| Menu Vai: indietro, avanti, simboli, definizione, problema successivo | VS Code | fatto | 8 |
| Tasto destro sull'icona nel Dock: i progetti recenti | macOS | fatto | 8 |
| Rispetta «Riduci movimento», «Aumenta contrasto» e «Riduci trasparenza» di macOS | macOS | fatto per «Riduci movimento» e «Aumenta contrasto»; Keel non usa trasparenze, quindi «Riduci trasparenza» non cambia niente | 8 |
| Nei campi di testo funzionano Ctrl+A, Ctrl+E e Ctrl+K, come in tutto il Mac | macOS | fatto | 8 |

### Movimento e rifinitura

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Nella vista Git, un file messo in stage scivola nella sua sezione invece di sparire | Linear, Things | fatto | 9 |
| I numeri dei contatori cambiano con un breve scorrimento | Apple | fatto | 9 |
| Le liste che si caricano mostrano righe grigie che pulsano, non uno spazio vuoto | GitHub, Linear | fatto | 9 |
| Raggio e spazio dei pannelli regolabili nello stile a card, come la nuova «Modern UI» di VS Code 1.147 | VS Code | fatto | 9 |

---

## L'ordine dei lavori

Ho deciso io, come mi hai chiesto. Si comincia dalle sei cose che hai chiesto tu, poi le differenze, poi il resto partendo da quello che userai di più. Ogni giro si chiude come le fasi del piano: test automatici, immagini da `scripts/snapshots.sh`, le prove a mano per te e un commit su `develop`.

**Giro 1 — Esplora risorse e Git che si parlano** · fatto
- **Cosa:**
  - lo scorrimento che resta;
  - l'albero che segue anche le differenze e scorre fino al file;
  - i colori di git su albero, cartelle e schede;
  - il salvataggio automatico come in Cursor;
  - il menu File completo;
  - avvisi, finestra degli errori, campanella e notifiche di macOS.
- **Fatto quando:** scorri in fondo all'albero, vai in Git, clicchi un file cambiato e torni ai file: sei sul file giusto, alla stessa altezza. Il file ha la sua M, la cartella il suo pallino. Fai un push e vedi «Push di N commit»; ne fai uno che deve fallire e la finestra ti dice perché e cosa fare.

**Giro 2 — Le differenze nuove** · fatto
- **Cosa:**
  - la vista delle differenze rifatta con l'editor di Keel, affiancata o in linea;
  - parole evidenziate, righe nascoste da aprire, colonna di destra modificabile;
  - Stage e Annulla per blocco, F7, i segni nella barra;
  - tutte le modifiche in una pagina;
  - la differenza dentro l'editor dalla barra colorata.
- **Fatto quando:** rivedi e metti in stage un commit intero senza uscire dalle differenze.

**Giro 3 — Git di tutti i giorni** · fatto
- **Cosa:**
  - Annulla l'ultimo commit;
  - storico dei messaggi e i limiti 50 e 72;
  - fetch da solo e commit in arrivo;
  - cambio di branch con le modifiche;
  - stash, pubblica il branch;
  - autore della riga accanto al cursore e link su GitHub;
  - cronologia locale dei file.
- **Fatto quando:** per una settimana non apri mai il terminale per git.

**Giro 4 — Navigare** · fatto
- **Cosa:**
  - indietro e avanti;
  - Ctrl+Tab e i luoghi recenti;
  - Cmd+P con anteprima, `:riga`, `@`, `#` e `>`;
  - sbirciare definizioni e riferimenti;
  - la struttura del file;
  - le schede con il nome della cartella e quelle che restano in vista;
  - la ricerca in una scheda.
- **Fatto quando:** salti avanti e indietro tra definizioni senza perdere il filo.

**Giro 5 — L'Esplora risorse completo** · fatto
- **Cosa:**
  - più file selezionati, copia e incolla, Cmd+Z;
  - cerca scrivendo;
  - cartelle appiccicate, guide, cartelle compatte e file annidati;
  - trascinare da e verso il Finder e il terminale;
  - import aggiornati quando sposti un file.
- **Fatto quando:** riorganizzi le cartelle di un progetto vero senza uscire da Keel e senza rompere un import.

**Giro 6 — Scrivere** · fatto
- **Cosa:**
  - tag rinominati in coppia, parentesi colorate, testo racchiuso;
  - spazi visibili e righelli;
  - suggerimenti in riga, colori, «N riferimenti»;
  - le occorrenze nella barra;
  - i comandi sulle righe.
- **Fatto quando:** scrivendo un componente React non ti manca niente di Cursor, Tab a parte.

**Giro 7 — Terminale** · fatto
- **Cosa:** integrazione con la shell (pallini, salti fra comandi, comando appiccicato, correzioni rapide), avanzamento, notifiche, terminali che tornano, «Chiedi a Claude».
- **Fatto quando:** un comando fallito si trova, si copia e si passa a Claude in due tasti.

**Giro 8 — Il Mac per davvero e la barra di stato** · fatto
- **Cosa:**
  - icona del file nel titolo e pallino delle modifiche;
  - menu Finestra e Aiuto, menu del Dock;
  - Quick Look, trascinare fuori da Keel;
  - le impostazioni di accessibilità di macOS;
  - la barra di stato cliccabile.
- **Fatto quando:** Keel si comporta come un'app di Apple in ogni gesto che fai senza pensarci.

**Giro 9 — Movimento e il resto** · fatto
- **Cosa:**
  - le animazioni delle liste e dei contatori;
  - cursore e scorrimento morbidi;
  - immagini nelle differenze e codice spostato;
  - segnalibri, appunti con la cronologia, i dettagli di Claude;
  - raggio e spazio dei pannelli regolabili.
- **Fatto quando:** lo decidi tu, usandolo.

Il giro 1 lo comincio appena mi dai il via, come vuole il piano: una cosa alla volta.

---

## Fonti

- [VS Code: Tips and Tricks](https://code.visualstudio.com/docs/getstarted/tips-and-tricks)
- [VS Code: User Interface](https://code.visualstudio.com/docs/getstarted/userinterface)
- [VS Code: Staging and committing changes](https://code.visualstudio.com/docs/sourcecontrol/staging-commits)
- [VS Code: Source Control](https://code.visualstudio.com/docs/sourcecontrol/overview)
- [VS Code 1.147, novembre 2026](https://code.visualstudio.com/updates)
- [VS Code 1.88](https://code.visualstudio.com/updates/v1_88): Stage e Annulla nella vista delle differenze; [1.81](https://code.visualstudio.com/updates/v1_81) e [1.82](https://code.visualstudio.com/updates/v1_82): righe uguali ripiegate e codice spostato
- [Zed: Git](https://zed.dev/docs/git)
- [Zed: le impostazioni](https://zed.dev/docs/configuring-zed)
- [Cursor: changelog](https://cursor.com/changelog) e [Inline edit](https://cursor.com/docs/inline-edit/overview): Cursor ha tolto il Cmd+K in riga, ora la selezione va all'agente
- [GitHub Desktop: commit e revisione delle modifiche](https://docs.github.com/en/enterprise-server@3.19/desktop/making-changes-in-a-branch/committing-and-reviewing-changes-to-your-project-in-github-desktop)
