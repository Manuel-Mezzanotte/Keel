# Keel — piano della messa online

*Scritto il 23 novembre 2026. Stato: piano, niente è ancora fatto. Si parte solo col via dell'autore.*

Lo scopo è rendere Keel scaricabile da chiunque e fare in modo che riceva gli aggiornamenti da solo, **senza pagare l'account sviluppatore di Apple** per ora. La firma di Apple si potrà aggiungere dopo senza rifare niente.

---

## Le decisioni prese

- **Due modi per installarlo,** spiegati in una pagina di download del sito:
  - **Script di installazione, il modo principale:**
    ```bash
    curl -fsSL https://<dominio>/install.sh | sh
    ```
    macOS mette l'etichetta «quarantena», che fa comparire l'avviso, solo ai file scaricati dal browser. Quello che scarica `curl` non ce l'ha, quindi Keel si apre senza avvisi.
  - **DMG, la seconda scelta,** per chi non usa il terminale. La prima volta va aperto da Impostazioni di Sistema → Privacy e sicurezza → «Apri comunque». Dal macOS 15 il tasto destro → Apri non basta più.
- **Aggiornamenti automatici con Sparkle,** il sistema delle app per Mac distribuite fuori dall'App Store, con l'avviso disegnato da Keel.
- **Niente App Store:** la sua sandbox vieta terminale, language server, debugger e accesso libero alle cartelle.
- **Homebrew** viene dopo: quello ufficiale ormai vuole app notarizzate da Apple.
- **Firma di Apple (Developer ID e notarizzazione, 99 $ l'anno):** rimandata. Quando arriverà, il DMG tornerà il modo principale e lo script resterà come alternativa.

## Come funzionano gli aggiornamenti

1. **Keel sa la sua versione.** È nel suo `Info.plist`:
   - `CFBundleShortVersionString`, quella che si legge, come `0.2.0`;
   - `CFBundleVersion`, un numero di build che cresce sempre.

   Oggi `scripts/bundle-macos.sh` mette in tutti e due la versione di `Cargo.toml` (0.1.0). Il numero di build va separato.
2. **Un file fisso online,** `https://<dominio>/appcast.xml`, elenca le versioni. Per ognuna: numero, data, novità in italiano e in inglese, indirizzo del pacchetto e firma.
3. **Keel legge quel file** quando la finestra è già sullo schermo, poi ogni qualche ora, fuori dal thread dell'interfaccia. Se il numero di build più alto del file è maggiore del suo, scarica l'aggiornamento in sottofondo, possibilmente solo la parte cambiata.
4. **Controlla la firma.** Ogni pacchetto è firmato con una chiave EdDSA dell'autore: gratis, diversa da quella di Apple, tenuta fuori dalla repo. Un pacchetto con una firma sbagliata non si installa.
5. **Mostra un avviso discreto:** «Keel 0.3 è pronto. Riavvia per aggiornare». L'app si sostituisce solo al riavvio e non interrompe mai il lavoro. Impostazioni, estensioni e il futuro `style.css` stanno nella cartella dei dati, quindi l'aggiornamento non li tocca.
6. **Senza rete** il controllo fallisce in silenzio e riprova più tardi.

Nelle Impostazioni ci saranno «Aggiorna automaticamente», da poter spegnere, e il comando «Cerca aggiornamenti» nel menu Keel.

## Le sottofasi

1. **Versioni e numero di build.**
   - Il numero di build va in `CFBundleVersion`, separato dalla versione.
   - La finestra «Informazioni su Keel» mostra versione e build.
2. **Sparkle dentro Keel.**
   - `Sparkle.framework` va in `Keel.app/Contents/Frameworks`, ed è chiamato da Rust con `objc2`.
   - L'avviso è disegnato da Keel tramite il suo «user driver».
   - L'impostazione `update.mode` e il comando «Cerca aggiornamenti».
   - Test con un appcast finto in locale: versione più nuova, uguale o più vecchia, firma sbagliata, nessuna rete.
3. **Script di rilascio** `scripts/release.sh`, un comando solo:
   1. alza la versione e il numero di build;
   2. costruisce Keel;
   3. crea il DMG e lo `.zip` per Sparkle;
   4. firma l'aggiornamento con la chiave;
   5. calcola le impronte SHA-256;
   6. scrive la voce nuova dell'appcast con le novità;
   7. carica tutto.
4. **Script di installazione** `install.sh`. È corto e leggibile, e fa:
   1. controlla di essere su un Mac con Apple Silicon e macOS 13 o successivo;
   2. scarica l'ultima versione;
   3. controlla l'impronta SHA-256;
   4. la mette in `/Applications`, o in `~/Applications` se manca il permesso, senza cancellare una versione già installata finché quella nuova non è al suo posto;
   5. apre Keel.
5. **Pagina di download su KeelSite** (`/download` e `/en/download`):
   - in evidenza la riga dello script, con «Copia» e il collegamento per leggerlo;
   - sotto, «Oppure scarica il DMG» con i passaggi per «Apri comunque» e le immagini delle Impostazioni;
   - il testo, onesto e senza spaventare: «Keel non è ancora firmato da Apple. Non vuol dire che sia pericoloso, solo che Apple non l'ha controllato»;
   - versione attuale, data, peso, requisiti (macOS 13 o successivo, Apple Silicon) e impronta SHA-256;
   - i pulsanti della pagina principale portano qui. `downloadUrl` in `src/site.ts` diventa l'indirizzo della pagina.
6. **Prova vera su un Mac pulito,** o con un utente nuovo:
   - installazione con lo script e con il DMG;
   - aggiornamento da una versione alla successiva, controllando che Sparkle non ripeta l'avviso di macOS;
   - misure di memoria e avvio prima e dopo Sparkle, con lo stesso metodo del confronto.
7. **Documenti:**
   - nel README, la sezione «Privacy», che deve dire che Keel controlla gli aggiornamenti su quell'indirizzo, e le istruzioni di installazione;
   - `HANDOFF.md`;
   - le domande del sito.

## Da decidere con l'autore

- **Il dominio del sito e dei download,** per esempio un sottodominio di quello che c'è già.
- **Dove mettere i pacchetti.** La repo di Keel è privata, quindi le sue Releases non sono scaricabili da tutti. Le scelte sono:
  - una repo pubblica fatta solo per i rilasci, con le GitHub Releases, gratis;
  - Cloudflare R2, gratis fino a 10 GB e senza costi di traffico.
- **Dove pubblicare il sito:** Cloudflare Pages, GitHub Pages o il mini PC con il tunnel.
- **La licenza** di Keel: oggi non ce n'è una, quindi tutti i diritti sono riservati.
- **Con che frequenza** cercare gli aggiornamenti: all'avvio e ogni 6 ore, oppure ogni 24.
- **Le novità di ogni versione:** scritte a mano oppure ricavate dai commit.

## Rischi

| Rischio | Cosa facciamo |
|---|---|
| Qualcuno teme lo script `curl … \| sh` | Lo script è corto, leggibile al suo indirizzo, e controlla l'impronta; il DMG resta come alternativa |
| Senza notarizzazione Sparkle ripete l'avviso di macOS dopo un aggiornamento | Si prova nella sottofase 6 prima di andare online; se succede, Keel toglie da solo l'etichetta di quarantena al pacchetto scaricato e verificato |
| Si perde la chiave di firma degli aggiornamenti | Copia nel portachiavi e una di riserva fuori dal Mac: senza, gli utenti non riceverebbero più aggiornamenti |
| Un aggiornamento rotto arriva a tutti | La versione si prova sul Mac dell'autore prima di entrare nell'appcast; una voce si può togliere dall'appcast in un attimo |
| macOS cambia le regole per le app non firmate | È il motivo per passare poi alla firma di Apple; tutto il resto non cambia |
