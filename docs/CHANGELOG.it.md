# Novità

Cosa cambia in ogni versione di Keel, dalla più recente. Keel si aggiorna
da solo: le novità che mostra nell'aggiornamento vengono da qui, più brevi.

*In English: [CHANGELOG.md](../CHANGELOG.md).*

## 0.1.3 — 27 novembre 2026

### Claude

- **Le chat degli altri progetti si aprono qui,** nella finestra dove
  lavori, con Claude che lavora nella loro cartella: una fascia sopra la
  chat dice dove, con «Apri il progetto». «+» torna a questo progetto.
- **Scrivere mentre Claude lavora:** il messaggio aspetta sopra il campo e
  parte appena il turno finisce; più messaggi partono insieme. Esc nel
  campo ferma Claude.
- **La chat aperta torna** quando riapri il progetto.
- **Riportare il codice a un messaggio:** da un tuo messaggio, ogni file
  cambiato da quel turno in poi torna com'era; un file creato da Claude va
  nel Cestino. Da una risposta, **continua in una chat nuova** da lì.
- **Il piano in una scheda:** quando Claude chiede «Ready to code?», il
  piano si apre nell'editor, da leggere intero, cambiare e annotare;
  «Rivedi con le mie note» glielo rimanda.
- **Commenti sulle modifiche di Claude:** nella scheda di una modifica, il
  numero di una riga o il 💬 di un blocco apre un commento; i commenti si
  vedono sotto le loro righe e partono insieme, ognuno con il suo codice.
- **La dettatura** nel campo: il microfono, o ⌘D.
- **Rinominare una chat, copiare una risposta,** e «Aggiungi a Claude» col
  tasto destro sui file dell'albero.
- **I nomi dei modelli leggibili** («Opus 5.5»), e il Mac resta sveglio
  mentre Claude lavora (Impostazioni › File e finestre).

### Le icone

- **Material Icon Theme dentro Keel:** i file secondo nome ed estensione
  (`.tsx` con l'atomo di React, `package.json`, `Dockerfile`…) e le
  cartelle aperte o chiuse, dal primo avvio, nell'albero, nelle schede, in
  Git, nella ricerca, in Apri file, in Problemi e nel pannello di Claude.
  I simboli semplici restano come «Icone semplici».
- **I progetti con la loro immagine** (la loro icona, la favicon o il logo)
  nei progetti recenti e nelle chat di Claude.

### La pagina accanto al codice

- **Una pagina web in una scheda:** la pagina del progetto
  (`localhost:3000`) si apre in Keel, da Porte o dalla palette, con avanti,
  indietro, ricarica, la larghezza di un telefono e di un tablet,
  l'Inspector di Safari, «Apri nel browser» e «Mostra a Claude», che mette
  un'immagine della pagina nel messaggio dopo. È il WebKit di macOS: i
  suoi processi esistono solo mentre una pagina è aperta.

### Git

- **Cosa cambia il ramo** rispetto a `main`, in una pagina, prima di aprire
  una pull request.
- **Il messaggio del commit in una finestra grande:** Maiusc+Esc nel
  messaggio, o ⤢ sotto; ⌘↩ fa il commit da lì.
- **I file trascinati dalla vista Git** nel terminale (il percorso),
  nell'editor (si aprono) o nella chat di Claude.

### Cose più piccole

- **Apri file (⌘P):** → nel campo vuoto, o ⌥↩, apre un file dietro
  l'elenco e lascia l'elenco aperto.
- **«Non mostrare più»** sugli avvisi che si ripetono (estensioni,
  programmi nel terminale, comandi lunghi, Claude); un comando li fa
  tornare.
- **⌘ con la rotella** ingrandisce o rimpicciolisce il testo del terminale.

### Corretto

- I test di un crate in una sottocartella di un progetto senza workspace
  Cargo non partivano.

## 0.1.2 — 25 novembre 2026

### Il pannello di Claude, rifatto

- **Accesso e uscita dal pannello.** Senza un login il pannello offre
  l'abbonamento Claude (Pro, Max, Team, Enterprise) o la Console Anthropic:
  parte il login di Claude Code in un terminale e si apre il browser; la
  chat si apre appena il login arriva. Il menu dell'account mostra piano ed
  email, ed esce.
- **Una chiave API tua,** tenuta nel Portachiavi di macOS, mai in un file,
  e data a Claude Code quando parte. Si può dimenticare.
- **Modello, ragionamento e modo in schede** sopra il messaggio: prima i
  modelli principali, i vecchi in «Altri modelli»; il ragionamento su un
  cursore da trascinare, con un nome per ogni livello e «Auto» come
  interruttore; la modalità veloce; il modo (Manuale, Accetta modifiche,
  Piano, Automatico, Senza permessi) con una riga su cosa fa ognuno.
- **`/` per comandi e skill:** all'inizio del messaggio, i comandi di Claude
  Code e le tue skill, filtrati mentre scrivi.
- **`@` per file e cartelle:** ovunque nel messaggio, la selezione, i file
  aperti e qualsiasi file o cartella del progetto, con la ricerca fuzzy.
- **Contesto e limiti del piano:** un anello nel campo di scrittura dice
  quanto è pieno il contesto della chat; la sua scheda mostra i token e i
  limiti delle 5 ore e della settimana con quando si azzerano, letti con il
  `/usage` di Claude Code senza consumare token. Con una chiave API, il
  costo della chat.
- **Chat in comune con Claude Code e con l'app Claude:** una chat
  continuata nel terminale, o altrove, si rilegge quando ci torni. Le chat
  dell'app Claude compaiono con il loro titolo, in una scheda per progetto,
  prima il progetto aperto; una chat di un altro progetto si apre nella
  finestra di quel progetto.
- **Eliminare una chat** dal cestino, con un secondo clic per conferma.
- **Aspetto nuovo:** il segno di Claude, una pagina iniziale con domande
  pronte, i tuoi messaggi in fumetti, gli strumenti come righe leggere (i
  comandi su più righe non si sovrappongono più alle righe sotto, e una
  riga aperta non sparisce più in una chat scorsa), il ragionamento su una
  riga che si apre leggibile, il piano con l'avanzamento, i permessi in un
  riquadro loro.

### I primi minuti

- **Importa da VS Code**, come da Cursor: impostazioni, scorciatoie,
  frammenti, progetti recenti ed estensioni. Niente di Keel viene
  sovrascritto.
- **«Prepara Keel»** dopo il benvenuto del primo avvio: il tema, l'import
  dagli editor trovati sul Mac, il comando `keel`.
- **Il comando `keel` nel Terminale:** `keel .` apre la cartella in Keel e
  restituisce subito il prompt, come `code .`.

### Corretto

- Keel fermo, con un file aperto, usava circa il 7% di un core e teneva la
  sua memoria grafica: ora non usa niente, e la memoria è scesa da 272 a
  157 MB. Il cursore smette di lampeggiare dopo 10 secondi senza scrivere.

## 0.1.1 — 24 novembre 2026

- **Impostazioni ridisegnate:** ogni pagina con la sua icona, una ricerca
  su tutte le impostazioni (nome, descrizione e chiave), riquadri per
  gruppo, un segno su quelle cambiate e un pulsante per tornare al valore
  di Keel.
- **Scorciatoie da tastiera** con la ricerca, i tasti disegnati come tasti e
  i pulsanti solo sulla riga sotto il puntatore.
- **Pagina di benvenuto** con il marchio di Keel, le azioni e i progetti
  recenti in due colonne e un mare calmo in fondo.
- **Un benvenuto per il primo avvio:** cielo, mare e la barca di Keel, poi
  «Sali a bordo». Aiuto › Rivedi il benvenuto lo fa rivedere.
- Gli avvisi delle licenze del software libero dentro Keel viaggiano con
  l'app, in Impostazioni › Aggiornamenti.

## 0.1.0 — 23 novembre 2026

La prima versione pubblica: editor, Git con il grafico, terminale,
debugger, test, pull request e Claude Code in un'app sola per il Mac. Da
qui in poi gli aggiornamenti arrivano da Keel stesso.
