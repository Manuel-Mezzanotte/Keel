# Keel — i dettagli

I piccoli dettagli che rendono comodo un IDE: quelli a cui non pensi finché mancano. Raccoglie cosa fanno VS Code, Cursor, Zed, JetBrains, Xcode, Sublime Text, i programmi per git e il Mac stesso, confrontato con quello che Keel fa oggi. Alla fine c'è l'ordine in cui farli, deciso da me.

*Scritto il 19 novembre 2026, sul commit `12db1b4` di `develop`. Stato: nessun giro ancora cominciato.*

---

## Come è fatto questo documento

- **Ricerca.** Ho letto le pagine ufficiali di VS Code (la 1.147 è uscita ieri), di Zed e di Cursor. Quello che riguarda JetBrains, Xcode, Sublime, GitHub Desktop e macOS viene da quello che so di questi programmi, non da pagine lette oggi. I link sono in fondo.
- **Confronto con Keel.** Per ogni punto ho guardato nel codice se c'è già. Nella colonna «Keel oggi»:
  - **rotto** vuol dire che c'è ma si comporta male;
  - **a metà** vuol dire che ne manca un pezzo;
  - **manca** vuol dire che non c'è.

  Le cose che Keel fa già bene non sono elencate. In tutto sono 143 punti: 6 rotti, 13 a metà, 124 che mancano.
- **Giro.** Il numero dice in quale giro di lavoro lo farei. I giri sono descritti alla fine, in «L'ordine dei lavori».
- **Fuori da qui.** Le funzioni grandi di cui abbiamo già parlato (commit scritto da Claude, chat in parallelo, pull request, Linear, cronologia locale, Docker) hanno il loro spazio. Qui ci sono solo quelle che servono a quelle grandi, come la cronologia locale per non perdere niente.

---

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
| Lo scorrimento resta quando cambi vista e quando riapri il progetto | VS Code, Zed | rotto | 1 |
| Il file rivelato si vede: l'albero scorre fino a lui, centrato, solo se è fuori vista | VS Code | a metà | 1 |
| Segue le differenze, Git Graph, la ricerca, i Problemi e i link del terminale | VS Code | manca | 1 |
| Colori e lettere di git, propagati alle cartelle con un pallino | VS Code, Zed, JetBrains | manca | 1 |
| Errori e avvisi propagati alle cartelle | VS Code | a metà: solo i file | 1 |
| Selezione di più file con Cmd e Maiusc, per spostarli, eliminarli o copiarli insieme | VS Code | manca | 5 |
| Copia, taglia, incolla e duplica (Cmd+C, Cmd+X, Cmd+V); «nome copia.ts» se il nome c'è già | VS Code | manca | 5 |
| Cmd+Z annulla l'ultima rinomina, spostamento o eliminazione | VS Code | manca | 5 |
| Scrivi per cercare nell'albero (Cmd+Alt+F): evidenzia o filtra, e le cartelle dicono quanti ne contengono | VS Code | manca | 5 |
| Le cartelle che contengono la riga in cima restano appiccicate in alto mentre scorri | VS Code, Zed | manca | 5 |
| Guide di rientro, più chiare al passaggio del mouse | VS Code, Zed | manca | 5 |
| Cartelle compatte: `src/main/java` su una riga sola, se ognuna contiene solo la successiva | VS Code, Zed | manca | 5 |
| File annidati: `package-lock.json` sotto `package.json`, `x.test.ts` e `x.css` sotto `x.ts` | VS Code | manca | 5 |
| Alt+clic apre di lato; Cmd+Invio apre a destra | VS Code | manca | 5 |
| Tasto destro: «Cerca nella cartella…», «Apri nel terminale», «Confronta con il selezionato» | VS Code | manca | 5 |
| Trascinare dal Finder copia i file nel progetto | VS Code | manca | 5 |
| Trascinare un file nel terminale scrive il suo percorso; nella chat di Claude lo allega | VS Code, Cursor | manca | 5 |
| Tenendo un file trascinato sopra una cartella chiusa, la cartella si apre | Finder, VS Code | manca | 5 |
| Trascinare un file dall'albero al Finder, a Mail o a Slack | macOS | manca | 8 |
| Spazio su un'immagine, un PDF o un video: anteprima Quick Look, come nel Finder | macOS | manca | 8 |

### Schede e gruppi

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Colori di git e degli errori anche sul nome della scheda | VS Code | manca | 1 |
| La scheda delle differenze con l'icona del suo linguaggio | — | rotto | 2 |
| Due file con lo stesso nome: la scheda aggiunge la cartella, `index.ts · components` | VS Code | manca | 4 |
| Aprendo un file la barra delle schede scorre fino a lui, se era nascosto | VS Code | manca | 4 |
| Clic con il tasto centrale chiude; doppio clic sullo spazio vuoto crea un file | VS Code | manca | 4 |
| «Chiudi i salvati» (Cmd+K U) | VS Code | manca | 4 |
| Ctrl+1…9 va alla scheda numero 1…9 | VS Code, Safari | manca | 4 |
| Schede su più righe quando non ci stanno, a scelta (`workbench.editor.wrapTabs`) | VS Code | manca | 9 |
| Trascinare una scheda fuori dalla finestra apre una finestra nuova | VS Code | manca | 8 |

### Salvataggio e file

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Salvataggio automatico acceso di serie, con la spunta nel menu File | VS Code, Cursor | a metà | 1 |
| «Cambiando editor» salva anche andando nel terminale, nella barra o in Claude | VS Code, Zed | rotto | 1 |
| Salva prima di avviare uno script npm o un'attività | — | manca | 1 |
| Menu File completo: Salva con nome, Salva tutto (Cmd+Alt+S), Ripristina il file, Chiudi la cartella | VS Code | manca | 1 |
| Cronologia locale: ogni salvataggio conservato, con una linea del tempo per file da cui tornare indietro, anche per un file cancellato | VS Code, JetBrains | manca | 3 |
| Confronta il file con gli appunti (Cmd+K C) | VS Code | manca | 3 |

### Git: la vista e il commit

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Avvisi dopo push, pull, fetch, commit, stash e cambio di branch | JetBrains, Tower | manca | 1 |
| Finestra d'errore con il messaggio di git e l'azione che risolve | VS Code, JetBrains | manca | 1 |
| «Annulla l'ultimo commit» subito dopo il commit, e dal menu | GitHub Desktop, Zed, VS Code | manca | 3 |
| Campo del commit: ↑ e ↓ ripescano i messaggi precedenti | VS Code | manca | 3 |
| Campo del commit: avviso oltre i 50 caratteri nel titolo e i 72 nel corpo, con il segno dei 72 | VS Code, Zed | manca | 3 |
| Niente in stage: chiede «Metto in stage tutto e faccio il commit?», con «Sempre» | VS Code | manca | 3 |
| Commit su `main` o `master`: chiede conferma | VS Code | manca | 3 |
| Commit e push in un gesto, scelto come azione predefinita del pulsante | VS Code, JetBrains | a metà: c'è nel menu | 3 |
| Prima del push, l'elenco dei commit che stanno per partire | JetBrains | manca | 3 |
| Fetch da solo ogni 3 minuti, «ultimo fetch 2 minuti fa», e il numero dei commit in arrivo | VS Code, GitHub Desktop | manca | 3 |
| Cambio di branch con modifiche non salvate in un commit: «Portale con te» o «Lasciale qui» (stash) | GitHub Desktop | manca | 3 |
| Elenco degli stash, con Applica, Estrai, Elimina e le differenze | VS Code, Zed | manca | 3 |
| Branch nuovo senza remoto: «Pubblica il branch» al posto di Push, poi «Crea pull request» | VS Code, GitHub Desktop | manca | 3 |
| Vista ad albero o a elenco dei file cambiati | VS Code, Zed | manca | 3 |
| Commit con il campo più grande in una scheda (Maiusc+Esc), per i messaggi lunghi | Zed, VS Code | manca | 3 |

### Git: dentro l'editor

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Chi ha cambiato la riga del cursore, in grigio a fine riga: «Manuel, 3 giorni fa · fix: …» | VS Code, Zed | manca: c'è solo la colonna | 3 |
| Passando sopra: il messaggio intero, l'autore, «Apri il commit» e «Apri su GitHub» | VS Code, JetBrains | manca | 3 |
| Clic sulla barra colorata a sinistra: la differenza dentro l'editor, con Stage, Annulla, Successiva, Precedente | VS Code | a metà: c'è la barra dei pulsanti | 2 |
| «Copia il link su GitHub» della riga o della selezione | Zed | manca | 3 |
| Storia delle righe selezionate (`git log -L`) | JetBrains | manca | 3 |

### Git: le differenze

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Colonne a metà, con il divisorio trascinabile | VS Code | rotto | 2 |
| Intestazioni dei blocchi leggibili, con la funzione in cui si trovano | GitHub | rotto | 2 |
| Righe nascoste da aprire: 20 in su, 20 in giù o tutte | VS Code, GitHub | manca | 2 |
| Parole cambiate evidenziate dentro la riga | Zed, VS Code | manca | 2 |
| Colonna di destra modificabile, con il salvataggio | VS Code, Zed | manca | 2 |
| Stage e Annulla per ogni blocco, accanto ai numeri di riga | VS Code | manca | 2 |
| F7 e Maiusc+F7 per passare da una modifica all'altra; i segni nella barra di scorrimento | VS Code | manca | 2 |
| In linea da sola quando la finestra è stretta (sotto i 900 pixel) | VS Code | manca | 2 |
| «Ignora gli spazi», per vedere solo i cambiamenti veri | VS Code, GitHub Desktop | manca | 2 |
| In cima: nome del file, «+12 −3» e «Apri il file» alla stessa riga | VS Code, GitHub | a metà | 2 |
| Tutte le modifiche in una sola pagina che scorre, file dopo file, modificabile | Zed, VS Code | manca | 2 |
| Immagini cambiate: prima e dopo affiancate, o una sopra l'altra con un cursore | GitHub Desktop | manca | 9 |
| Codice spostato riconosciuto come spostato, non come tolto e aggiunto | VS Code | manca | 9 |

### Navigare

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Indietro e avanti tra i punti visitati (Ctrl+- e Ctrl+Maiusc+-), anche con i tasti laterali del mouse | VS Code | manca | 4 |
| Torna all'ultima modifica (Cmd+K Cmd+Q) | VS Code, JetBrains | manca | 4 |
| Ctrl+Tab: elenco dei file recenti, tenendo Ctrl si scorre | VS Code | manca | 4 |
| Luoghi recenti: i punti dove sei stato, con un pezzetto di codice ciascuno | JetBrains | manca | 4 |
| Cmd+P: scorrendo l'elenco il file si vede già nell'editor, Esc torna dov'eri | Sublime, VS Code | manca | 4 |
| Cmd+P: `file.ts:42` apre alla riga 42; `@` i simboli del file, `#` quelli del progetto, `>` i comandi | VS Code, Sublime | manca | 4 |
| Cmd+P: freccia a destra apre il file in secondo piano e lascia aperto l'elenco | VS Code | manca | 4 |
| Sbirciare la definizione e i riferimenti in un riquadro dentro l'editor (Alt+F12, Maiusc+F12) | VS Code | manca | 4 |
| Gerarchia delle chiamate: chi chiama questa funzione, e chi chiama quelli | VS Code, JetBrains | manca | 4 |
| Struttura del file in una vista della barra laterale, che segue il cursore | VS Code, Zed | manca | 4 |
| Breadcrumb: un clic su un pezzo apre l'elenco dei fratelli, file o simboli | VS Code, Xcode | a metà: una cartella si apre nell'albero | 4 |
| Vai alla parentesi corrispondente (Cmd+Maiusc+\\) | VS Code | manca | 6 |
| Segnalibri sulle righe, con l'elenco per tornarci | JetBrains, Xcode | manca | 9 |

### Scrivere

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Rinominando un tag HTML o JSX si rinomina anche quello di chiusura | VS Code | manca | 6 |
| Parentesi colorate per livello, con la guida della coppia attiva | VS Code, Zed | manca | 6 |
| Selezioni del testo e scrivi `"` o `(`: il testo finisce dentro | VS Code, Sublime | manca | 6 |
| Spazi visibili nella selezione o ai bordi (`editor.renderWhitespace`) | VS Code, Zed | manca | 6 |
| Righelli verticali a 80 o 100 colonne (`editor.rulers`) | VS Code | manca | 6 |
| Suggerimenti in riga: tipi e nomi dei parametri, attenuati, tenendo premuto un tasto (`editor.inlayHints`) | VS Code, Zed | manca | 6 |
| Quadratino del colore accanto a `#2F74F5` o `rgb(…)`, con il selettore | VS Code | manca | 6 |
| «3 riferimenti» sopra le funzioni, cliccabile (CodeLens) | VS Code | manca | 6 |
| Le altre occorrenze del simbolo evidenziate, e segnate nella barra di scorrimento | VS Code, Zed | manca | 6 |
| Errori, risultati della ricerca e modifiche segnati nella barra di scorrimento | VS Code, Zed | manca: l'editor li sa disegnare, Keel non glieli passa | 6 |
| Cmd+C senza selezione copia la riga intera; Cmd+X la taglia | VS Code, Sublime | manca | 6 |
| Ordina le righe, togli i doppioni, unisci le righe, maiuscole e minuscole, snake e camel case | VS Code, Sublime | manca | 6 |
| Espandi e restringi la selezione per blocchi (Ctrl+Maiusc+Cmd+→ e ←) | VS Code, JetBrains | manca | 6 |
| Spostando un file, gli import che lo usano si aggiornano da soli (TypeScript) | VS Code | manca | 5 |
| Cursore che scivola invece di saltare, e scorrimento morbido | VS Code, Zed | manca | 9 |
| Incollare un'immagine in un Markdown la salva nel progetto e scrive il link | VS Code | manca | 9 |
| Appunti con la cronologia: Cmd+Maiusc+V incolla una delle ultime cose copiate | JetBrains | manca | 9 |

### Cerca

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| I risultati si aprono in una scheda che puoi tenere e modificare, come in Zed | Zed, VS Code | manca | 4 |
| «Cerca nella cartella…» dall'albero riempie «file da includere» | VS Code | manca | 5 |
| Cerca solo nei file aperti | VS Code | manca | 4 |
| Togli un risultato o un file dall'elenco con la ×, prima di sostituire | VS Code | manca | 4 |
| Il testo selezionato o la parola sotto il cursore riempie già il campo di ricerca | VS Code | manca | 4 |

### Terminale

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Un pallino accanto a ogni comando: blu se è andato bene, rosso se è fallito (integrazione con la shell) | VS Code | manca | 7 |
| Cmd+↑ e Cmd+↓ saltano da un comando all'altro | VS Code, iTerm | manca | 7 |
| Il comando in cima resta appiccicato mentre scorri il suo output | VS Code | manca | 7 |
| «Copia l'output dell'ultimo comando» e «Esegui un comando recente» | VS Code | manca | 7 |
| Correzioni rapide: porta occupata → «Chiudi il processo»; push senza upstream → «Imposta upstream» | VS Code | manca | 7 |
| Nome della scheda preso dal programma che gira (node, cargo, claude) | VS Code, iTerm | manca | 7 |
| Barra di avanzamento nella scheda, per i programmi che la mandano | VS Code, Ghostty | manca | 7 |
| Notifica quando un comando lungo finisce e Keel è in secondo piano | iTerm, Ghostty | manca | 7 |
| Riaprendo Keel, i terminali tornano con il loro testo | VS Code | manca | 7 |
| Selezione o comando fallito → «Chiedi a Claude» | Cursor | manca | 7 |

### Avvisi e avanzamento

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Avvisi in basso a destra, che entrano e escono con un movimento, impilati se sono più di uno | VS Code, Zed | manca | 1 |
| Ogni avviso con le sue azioni, e «Non mostrare più» per quelli che si ripetono | VS Code, JetBrains | manca | 1 |
| Campanella con gli avvisi passati e «Non disturbare» | VS Code | manca | 1 |
| Avanzamento nella barra di stato per git, i language server e le installazioni | VS Code | a metà | 1 |
| Notifiche di macOS solo quando Keel è in secondo piano | macOS | manca | 1 |

### Barra di stato

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Clic su riga e colonna → vai alla riga | VS Code, Zed | manca | 8 |
| Clic sul rientro → spazi o tab, 2 o 4, e converti il file | VS Code | manca | 8 |
| Clic sulla codifica → riapri o salva con un'altra codifica; oggi è sempre scritto «UTF-8» | VS Code | rotto | 8 |
| LF o CRLF, con il clic per cambiarlo | VS Code, Zed | manca | 8 |
| Clic sul linguaggio → cambialo per questo file | VS Code, Zed | manca | 8 |
| Quanti caratteri e righe sono selezionati, e quanti cursori ci sono | VS Code | manca | 8 |
| Dopo Cmd+K: «Cmd+K premuto, aspetto il secondo tasto» | VS Code, Zed | manca | 8 |
| Branch con ↓2 ↑3: un clic sincronizza, il cerchio gira mentre lavora | VS Code | a metà | 3 |

### Claude, i dettagli piccoli

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| I file creati o cambiati da Claude segnati nell'albero finché non li guardi | Cursor | manca | 9 |
| Barra di revisione in basso nell'editor: «file 1 di 3», Tieni e Annulla per ogni blocco e per tutto | Cursor | a metà | 9 |
| Trascinare un file o una cartella nella chat la allega; incollare un'immagine la allega | Cursor | manca | 9 |
| Problemi e test falliti: «Correggi con Claude» | Cursor | manca | 9 |
| Notifica di macOS e numero sul Dock quando Claude ha finito o aspetta un permesso | Cursor | manca | 8 |

### Il Mac per davvero

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Icona del file nel titolo: Cmd+clic mostra il percorso, trascinarla trascina il file | macOS, Xcode | manca | 8 |
| Pallino nel pulsante rosso della finestra quando ci sono modifiche non salvate | macOS | manca | 8 |
| Menu Finestra: Contrai (Cmd+M), Ingrandisci, l'elenco delle finestre aperte | macOS | manca | 8 |
| Menu Aiuto con la ricerca dei comandi, come in ogni app del Mac | macOS | manca | 8 |
| Menu Vai: indietro, avanti, simboli, definizione, problema successivo | VS Code | a metà | 8 |
| Tasto destro sull'icona nel Dock: i progetti recenti | macOS | manca | 8 |
| Rispetta «Riduci movimento», «Aumenta contrasto» e «Riduci trasparenza» di macOS | macOS | a metà: c'è l'interruttore dei movimenti | 8 |
| Nei campi di testo funzionano Ctrl+A, Ctrl+E e Ctrl+K, come in tutto il Mac | macOS | a metà: manca Ctrl+K | 8 |

### Movimento e rifinitura

| Cosa | Da dove | Keel oggi | Giro |
|---|---|---|---|
| Nella vista Git, un file messo in stage scivola nella sua sezione invece di sparire | Linear, Things | manca | 9 |
| I numeri dei contatori cambiano con un breve scorrimento | Apple | manca | 9 |
| Le liste che si caricano mostrano righe grigie che pulsano, non uno spazio vuoto | GitHub, Linear | manca | 9 |
| Raggio e spazio dei pannelli regolabili nello stile a card, come la nuova «Modern UI» di VS Code 1.147 | VS Code | manca | 9 |

---

## L'ordine dei lavori

Ho deciso io, come mi hai chiesto. Si comincia dalle sei cose che hai chiesto tu, poi le differenze, poi il resto partendo da quello che userai di più. Ogni giro si chiude come le fasi del piano: test automatici, immagini da `scripts/snapshots.sh`, le prove a mano per te e un commit su `develop`.

**Giro 1 — Esplora risorse e Git che si parlano**
- **Cosa:**
  - lo scorrimento che resta;
  - l'albero che segue anche le differenze e scorre fino al file;
  - i colori di git su albero, cartelle e schede;
  - il salvataggio automatico come in Cursor;
  - il menu File completo;
  - avvisi, finestra degli errori, campanella e notifiche di macOS.
- **Fatto quando:** scorri in fondo all'albero, vai in Git, clicchi un file cambiato e torni ai file: sei sul file giusto, alla stessa altezza. Il file ha la sua M, la cartella il suo pallino. Fai un push e vedi «Push di N commit»; ne fai uno che deve fallire e la finestra ti dice perché e cosa fare.

**Giro 2 — Le differenze nuove**
- **Cosa:**
  - la vista delle differenze rifatta con l'editor di Keel, affiancata o in linea;
  - parole evidenziate, righe nascoste da aprire, colonna di destra modificabile;
  - Stage e Annulla per blocco, F7, i segni nella barra;
  - tutte le modifiche in una pagina;
  - la differenza dentro l'editor dalla barra colorata.
- **Fatto quando:** rivedi e metti in stage un commit intero senza uscire dalle differenze.

**Giro 3 — Git di tutti i giorni**
- **Cosa:**
  - Annulla l'ultimo commit;
  - storico dei messaggi e i limiti 50 e 72;
  - fetch da solo e commit in arrivo;
  - cambio di branch con le modifiche;
  - stash, pubblica il branch;
  - autore della riga accanto al cursore e link su GitHub;
  - cronologia locale dei file.
- **Fatto quando:** per una settimana non apri mai il terminale per git.

**Giro 4 — Navigare**
- **Cosa:**
  - indietro e avanti;
  - Ctrl+Tab e i luoghi recenti;
  - Cmd+P con anteprima, `:riga`, `@`, `#` e `>`;
  - sbirciare definizioni e riferimenti;
  - la struttura del file;
  - le schede con il nome della cartella e quelle che restano in vista;
  - la ricerca in una scheda.
- **Fatto quando:** salti avanti e indietro tra definizioni senza perdere il filo.

**Giro 5 — L'Esplora risorse completo**
- **Cosa:**
  - più file selezionati, copia e incolla, Cmd+Z;
  - cerca scrivendo;
  - cartelle appiccicate, guide, cartelle compatte e file annidati;
  - trascinare da e verso il Finder e il terminale;
  - import aggiornati quando sposti un file.
- **Fatto quando:** riorganizzi le cartelle di un progetto vero senza uscire da Keel e senza rompere un import.

**Giro 6 — Scrivere**
- **Cosa:**
  - tag rinominati in coppia, parentesi colorate, testo racchiuso;
  - spazi visibili e righelli;
  - suggerimenti in riga, colori, «N riferimenti»;
  - le occorrenze nella barra;
  - i comandi sulle righe.
- **Fatto quando:** scrivendo un componente React non ti manca niente di Cursor, Tab a parte.

**Giro 7 — Terminale**
- **Cosa:** integrazione con la shell (pallini, salti fra comandi, comando appiccicato, correzioni rapide), avanzamento, notifiche, terminali che tornano, «Chiedi a Claude».
- **Fatto quando:** un comando fallito si trova, si copia e si passa a Claude in due tasti.

**Giro 8 — Il Mac per davvero e la barra di stato**
- **Cosa:**
  - icona del file nel titolo e pallino delle modifiche;
  - menu Finestra e Aiuto, menu del Dock;
  - Quick Look, trascinare fuori da Keel;
  - le impostazioni di accessibilità di macOS;
  - la barra di stato cliccabile.
- **Fatto quando:** Keel si comporta come un'app di Apple in ogni gesto che fai senza pensarci.

**Giro 9 — Movimento e il resto**
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
