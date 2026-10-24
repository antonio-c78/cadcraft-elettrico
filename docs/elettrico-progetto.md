# Struttura del progetto elettrico CADCraft

Stato: architettura definita il 24 ottobre 2026; prima implementazione di contenitore,
gestore fogli e rimandi in collaudo. Il registro fisico condiviso resta futuro.

Il progetto elettrico sarà un documento unico che raccoglie disegni, dati e risorse
necessarie al lavoro. L'utente aprirà, salverà e trasferirà un solo file. Il DXF
resterà il formato dei disegni interni e dello scambio con altri CAD.

Questa decisione prepara planimetrie, schemi unifilari, morsettiere e distinta
materiali nello stesso progetto. I DXF indipendenti continuano a usare il salvataggio
ordinario. Le sezioni seguenti definiscono l'architettura completa; la disponibilità
attuale è delimitata qui sotto.

## Prima versione per il collaudo

Elettrico → Progetto → Fogli di progetto apre il pannello con anteprime, proprietà,
apertura, duplicazione, ordinamento e rimozione dei fogli. Un DXF aperto si aggiunge
esplicitamente; i nuovi unifilari vengono aggiunti automaticamente al progetto aperto.
Chiudere una scheda conserva il foglio e le sue modifiche nella sessione.

Il `.cce` contiene manifest con impronte SHA-256, metadati e DXF dei fogli. Il
salvataggio verifica il file temporaneo, conserva il precedente in `.cce.bak` e
controlla che il file su disco non sia cambiato nel frattempo. Un `.lock` protegge
il salvataggio concorrente. Un blocco lasciato da un arresto viene segnalato,
non rimosso automaticamente. Salva con nome mantiene l'identità del progetto.

Il modello unifilare viene incorporato con le sue dipendenze dichiarate: cartiglio,
catalogo e libreria DXF. Si possono quindi inserire e modificare apparecchi senza
le cartelle originali. Queste definizioni rimangono congelate: la modifica grafica
del catalogo generale continua nel gestore, fuori dal progetto. L'acquisizione
autonoma delle definizioni del compositore e dei simboli di planimetria resta futura:
grafica e dati inseriti restano nei DXF, alcune modifiche richiedono il catalogo originale.

Rimando tra fogli chiede la destinazione e due punti; ogni estremo è un terminale
del motore dei fili. La coppia ha un UUID indipendente dai numeri di pagina.
Le etichette mostrano il foglio e, negli unifilari, la colonna. Rinumerare aggiorna
i rimandi e il campo foglio del cartiglio. Riordinare non rinumera implicitamente:
Rinumera in ordine è un'azione distinta. Doppio clic o Vai al rimando collegato
raggiunge e seleziona l'altro estremo.

I rimandi incompleti restano segnalati e possono essere completati. Annulla e
Ripristina agiscono sul singolo estremo. Duplicare un foglio genera nuove identità
dei rimandi, senza unirli ai circuiti originali. I tratti restano gestiti localmente
nei disegni; il registro globale di reti, cavi e materiali resta da implementare.

Limiti iniziali: 128 fogli, 1024 estremi di rimando, 2048 file, 32 MiB per file e
128 MiB complessivi anche dopo decompressione. I file non vengono estratti sul disco.
Percorsi non locali, collisioni, risorse alterate e funzionalità obbligatorie
sconosciute vengono rifiutati. Consultazione di versioni future in sola lettura,
nuovo progetto da copia e autorecupero completo della sessione `.cce` restano futuri.
Questa versione dispone di salvataggio esplicito e copia del progetto precedente.

## Contenitore e contenuti

L'estensione scelta per il progetto è `.cce` (CADCraft Elettrico). Il contenitore sarà un archivio
ZIP con file JSON UTF-8 e disegni DXF. La struttura rimarrà documentata e leggibile
con strumenti comuni; non richiederà un database installato o servizi online.
L'associazione dell'estensione a Windows avverrà con l'implementazione.

```text
Villetta.cce
  manifest.json
  project.json
  model.json
  drawings/
    <drawing_uuid>.dxf
  resources/
    symbols/<definition_uuid>/<revision>/definition.json
    symbols/<definition_uuid>/<revision>/geometry.dxf
    templates/<template_uuid>/<revision>/definition.json
    templates/<template_uuid>/<revision>/geometry.dxf
    assets/<sha256>.<extension>
  attachments/
    <attachment_uuid>.<extension>
  previews/
    <drawing_uuid>.png
  cache/
    index.json
```

| Contenuto | Responsabilità |
| --- | --- |
| `manifest.json` | Identità del formato, versioni, identità del progetto, generazione di salvataggio, elenco dei contenuti e impronte SHA-256 dei file necessari. |
| `project.json` | Cliente, impianto, commessa, quadri, piani, elenco e ordine delle tavole, unità, impostazioni condivise e registro delle revisioni. |
| `model.json` | Identità e relazioni elettriche condivise fra tavole, secondo le regole di autorità dei dati. Le sezioni non ancora implementate restano assenti. |
| `drawings/` | Geometria CAD, disposizione degli oggetti, testi, layer, percorsi grafici e metadati locali di ogni disegno. |
| `resources/` | Copie delle definizioni e delle risorse effettivamente utilizzate nel progetto. |
| `attachments/` | Documenti allegati dall'utente, per esempio la planimetria ricevuta dal cliente. |
| `previews/` e `cache/` | Anteprime e indici rigenerabili; la loro assenza non compromette il progetto. |

I nomi visibili delle tavole sono separati dai nomi dei file interni: rinominare
“Piano terra” non cambia l'identità del disegno o i riferimenti ad esso. Tutti i
percorsi necessari sono relativi al contenitore. Il nome e il percorso originari
di un allegato servono soltanto a identificarne la provenienza.

## Identità degli oggetti

Il progetto, ogni disegno, ogni oggetto grafico e ogni entità fisica condivisa
avranno un UUID permanente. Le sigle visibili, come QF1 o X1, sono dati modificabili
e non identificativi tecnici. Gli handle DXF continuano a essere usati localmente
dal motore; i collegamenti fra file useranno UUID.

Una rappresentazione grafica avrà un proprio `object_id` e, quando rappresenta un
oggetto fisico condiviso, un `component_id`. Più rappresentazioni possono riferirsi
allo stesso componente: il suo contributo alla distinta si conta una volta.
Annotazioni, richiami e simboli puramente grafici non diventano automaticamente
materiale da ordinare.

I punti di collegamento avranno identificativi stabili all'interno della
definizione del componente. Spostarne la grafica o cambiarne l'etichetta non
cambierà i riferimenti dei fili. La rimozione di un punto utilizzato richiederà
una rimappatura esplicita. IN e OUT descrivono punti utilizzabili senza imporre
un verso obbligatorio del collegamento.

Una copia reale di un componente riceverà nuovi identificativi, compresi quelli
dei suoi sottoelementi. Un richiamo dello stesso componente conserverà invece
il `component_id` e riceverà un nuovo `object_id`. Copiare un gruppo rimapperà
anche i riferimenti interni, preservando separatamente i collegamenti esterni.

## Autorità dei dati

Ogni campo avrà un unico proprietario modificabile. La prima versione del
contenitore conserverà i dati elettrici attuali nei DXF e aggiungerà il manifest,
i dati generali e le risorse. Non creerà una seconda copia modificabile di fili
e componenti in `model.json`.

Con l'introduzione del modello condiviso, una migrazione esplicita promuoverà le
entità interessate al registro di progetto. Da quel momento si applicherà questa
divisione:

| Dato | Origine autorevole |
| --- | --- |
| Cliente, quadri, piani, tavole e revisioni | `project.json` |
| Sigla e caratteristiche del componente fisico condiviso, articolo assegnato, composizione fisica e appartenenza funzionale | `model.json` |
| Morsetti fisici, collegamenti logici fra componenti, cavi e conduttori condivisi | `model.json` |
| Posizione, scala, rotazione, layer, posizione dei testi, perimetri degli ambienti e percorso grafico dei collegamenti | DXF della tavola |
| Definizione utilizzata di un simbolo o cartiglio | Revisione locale in `resources/` |
| Testi visualizzati, pallini, anteprime, quantità aggregate e indici | Proiezioni o risultati ricalcolabili |

Il DXF potrà conservare una proiezione dei dati condivisi per visualizzazione,
esportazione e recupero. Tale proiezione sarà identificata come derivata e legata
alla generazione del progetto. I comandi nel progetto aggiorneranno il proprietario
del dato e le sue rappresentazioni nella stessa transazione; non si sceglierà mai
in modo automatico il valore “più recente” fra copie discordanti.

Un ambiente geometrico appartiene alla sua planimetria. Le assegnazioni condivise
useranno il suo UUID, non soltanto il nome “Cucina”. Uno stato distinto indicherà
assegnazione manuale, automatica, assente o da verificare.

## Componenti e distinta materiali

Una scatola composta sarà un insieme fisico con sottoelementi identificabili:
scatola da incasso, supporto, frutti, coprifori e placca. La distinta deriverà da
questi elementi e dagli articoli assegnati, senza contare una seconda volta il
simbolo che rappresenta l'insieme.

Tipo della scatola, capacità del supporto, serie civile e ingombro dei frutti
saranno proprietà distinte. La scatola fisica a sei posti non verrà equiparata
automaticamente alla capacità di sei o sette moduli del supporto: la combinazione
dipenderà dalla serie e dalle regole di compatibilità del catalogo.

L'assegnazione di un articolo conserverà una copia dei dati commerciali utilizzati
e la provenienza della revisione. Gli aggiornamenti di catalogo o di listino
richiederanno un'azione esplicita. Le quantità aggregate saranno sempre ricavate
dalle istanze fisiche; eventuali aggiunte manuali saranno voci separate e riconoscibili.

Una modifica locale a una rappresentazione non dovrà creare implicitamente un
nuovo articolo o una seconda istanza fisica. Il progetto segnalerà dati mancanti
e associazioni da verificare, senza inventare componenti o quantità.

## Fili cavi morsetti e posa

Il modello distinguerà quattro elementi:

1. **Collegamento logico**: quali terminali o nodi appartengono alla stessa rete.
2. **Rappresentazione grafica**: i tratti e le giunzioni visibili in una tavola.
3. **Oggetto materiale**: cavo, conduttore, morsetto o altro articolo fisico.
4. **Percorso di posa**: tragitto reale, risalite e lunghezze dell'installazione.

La lunghezza disegnata di un filo nello schema unifilare non sarà usata come
lunghezza del cavo da ordinare. Una linea unifilare potrà rappresentare un cavo
con più conduttori; più tratti grafici potranno rappresentare lo stesso oggetto.
I metri proverranno da una misura di posa o da un valore inserito dall'utente,
con provenienza e unità registrate.

Una morsettiera potrà contenere morsetti con identità stabile, numero visibile,
articolo, terminali e ponticelli. Cambiare la numerazione non romperà i collegamenti.
Le pagine di riepilogo saranno rappresentazioni generate dagli stessi dati, non
elenchi indipendenti da mantenere a mano.

Il motore attuale dei collegamenti resterà responsabile dei percorsi locali,
delle giunzioni e delle loro modifiche. Un adattatore tradurrà i suoi riferimenti
locali nel registro condiviso senza sostituire il tracciamento già collaudato.

## Simboli e risorse del progetto

All'inserimento verrà acquisita una revisione locale della definizione utilizzata:
grafica, origine, punti di collegamento, regole dei testi, ingombri, moduli e
attributi necessari alla modifica. Le sole geometrie già disegnate non bastano a
garantire che un componente possa essere ricomposto su un altro computer.

Saranno incluse le risorse effettivamente necessarie, comprese quelle annidate,
con un elenco delle dipendenze e delle impronte. Risorse identiche potranno essere
condivise internamente, senza duplicarle per ogni istanza.

Il catalogo generale resterà esterno e continuerà a essere modificabile. Aprire
un progetto non aggiornerà i simboli già utilizzati. L'aggiornamento esplicito
mostrerà le differenze e controllerà i punti collegati prima dell'applicazione.
I percorsi di origine esterni non saranno necessari per riaprire e modificare
le funzionalità supportate da quella versione del progetto.

## Salvataggio copie e recupero

Il salvataggio acquisirà una generazione coerente di tutti i dati modificati,
scriverà un contenitore temporaneo nella cartella di destinazione, ne verificherà
contenuti, riferimenti e impronte, quindi sostituirà il file finale con l'operazione
atomica supportata dal sistema. Un errore prima della sostituzione manterrà il
progetto precedente. Le modifiche non ancora salvate resteranno disponibili in sessione.

Le copie di sicurezza e il recupero automatico saranno separati dal contenitore
principale, con conservazione limitata. Il pacchetto conserverà il registro delle
revisioni e le eventuali revisioni pubblicate richieste dall'utente; non accumulerà
ogni salvataggio come una copia completa interna.

“Salva con nome” manterrà l'identità del progetto. “Crea nuovo progetto da copia”
genererà nuove identità e rimapperà tutti i riferimenti interni. L'apertura di due
copie divergenti della stessa identità sarà riconosciuta: nessuna fusione automatica
o sovrascrittura silenziosa. L'accesso concorrente da altre istanze dovrà essere
gestito con blocco di scrittura e controllo della generazione salvata.

L'Annulla dei comandi elettrici coinvolgerà insieme disegni e dati di progetto.
Il comando di salvataggio userà sempre lo stesso stato coerente della sessione.

## Versioni importazione ed esportazione

Il manifest dichiarerà `format: cadcraft-electrical-project`, versione del
contenitore, versione del modello semantico, `project_id`, `save_generation`,
funzionalità richieste ed elenco dei file. Le impronte riguarderanno i contenuti
elencati, non il manifest stesso. Non costituiscono una firma di autenticità.

Le revisioni dei simboli e i formati delle loro definizioni avranno versioni
separate. Il lettore controllerà le funzionalità richieste prima di consentire
modifiche. Una versione più recente non riconosciuta non verrà risalvata con
perdita di dati: si userà, quando possibile, una consultazione senza modifica.

L'importazione di un DXF attuale creerà una copia di lavoro, conserverà i metadati
esistenti e assegnerà le identità mancanti. Gli handle locali verranno rimappati
nei nuovi riferimenti persistenti. La prima apertura non convertirà o sovrascriverà
il file originale. L'acquisizione delle risorse segnalerà quelle mancanti prima
di dichiarare il progetto completamente trasferibile.

L'esportazione DXF produrrà un disegno utilizzabile separatamente. Un file
esportato e modificato in un altro CAD rientrerà mediante un'importazione con
confronto, senza assumere che le identità o i metadati siano stati preservati.
Il supporto DWG richiederà prove proprie di conservazione dei dati; non sarà
considerato equivalente al DXF sulla sola base della somiglianza grafica.

Le estensioni dei moduli useranno spazi di nomi e versioni. I dati opzionali
sconosciuti saranno conservati senza essere interpretati; una funzionalità
obbligatoria non supportata impedirà il salvataggio modificabile del progetto.

## Validazione del contenitore

Il lettore applicherà limiti espliciti a numero dei file, dimensioni decompresse,
dimensioni JSON, entità CAD e riferimenti annidati, prima delle allocazioni. I
limiti attuali dei motori di disegno e di collegamento continueranno a valere.
Le soglie del contenitore saranno definite e collaudate con la prima implementazione.

Saranno rifiutati percorsi assoluti, uscite dalla cartella del progetto, collegamenti
simbolici, nomi duplicati o ambigui su Windows e riferimenti obbligatori mancanti.
Nessun file incluso verrà eseguito. Allegati, anteprime e contenuti necessari
saranno distinti: una miniatura mancante si rigenera, un disegno mancante richiede
recupero e non viene ignorato per consentire un salvataggio incompleto.

## Sequenza di implementazione

1. **Identità e compatibilità**: UUID di progetto, disegni e oggetti; adattatore
   dei metadati attuali; regole per copia, richiamo, importazione e riferimenti.
2. **Contenitore iniziale**: disegni DXF, dati generali, copie delle risorse usate,
   apertura e salvataggio coerenti, backup e recupero. Il DXF mantiene inizialmente
   l'autorità dei dati locali già presenti.
3. **Registro condiviso**: promozione esplicita delle entità fisiche interessate,
   più tavole, collegamenti fra rappresentazioni e Annulla comune.
4. **Moduli applicativi**: distinta e legenda, morsettiere, cavi e posa, riutilizzando
   identità e regole del progetto. I riepiloghi saranno ricostruibili dai dati originali.

Ogni fase richiederà prove di riapertura senza il catalogo generale originale,
copia su un altro percorso, simboli aggiornati esternamente, salvataggio interrotto,
identità duplicate e compatibilità con i disegni precedenti. Le fasi successive
aggiungeranno prove per rappresentazioni multiple, conteggio unico del materiale,
rimappatura dei terminali e recupero da dipendenze mancanti.

I futuri comandi di progetto saranno registrati nel motore e condivisi da menu,
barra strumenti, riga di comando e MCP. Il formato non richiederà finestre o
operazioni aggiuntive durante il normale disegno.

## Riferimenti della decisione

La guida locale di Sabik 30 documenta la tabella simboli interna allo schema,
la copia dei simboli dagli archivi allo schema corrente, le etichette locali,
l'archiviazione dei file di lavoro e l'elenco delle dipendenze nell'integrazione PDM.
Questi comportamenti orientano i requisiti di autonomia e portabilità, senza
definire il nostro formato o richiedere la riproduzione del formato SCH.

Nel codice attuale i blocchi DXF conservano la grafica; `CC_COMPOSITORE`,
`CC_SIMBOLO`, `CC_AMBIENTE`, `CC_AMBIENTE_REF`, `CC_UNIFILARE_DEVICE`,
`CC_UNIFILARE_LINK` e `CC_UNIFILARE_LAYOUT` conservano metadati specifici.
Le definizioni esterne sono ancora necessarie per alcune operazioni di modifica.
La struttura qui definita estende questa base senza dichiarare già disponibili
il contenitore, gli UUID globali o il registro condiviso.
