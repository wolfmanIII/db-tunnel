# Appunti Rust per db-tunnel

Pensati per chi programma da anni ma non conosce Rust. Ogni sigla è spiegata la prima volta che compare. Il file cresce un concetto alla volta.

## Glossario

- **Garbage collector (GC)**: meccanismo di Java, C#, PHP, Python, JS, Go che libera la memoria da solo mentre il programma gira. Rust non ce l'ha.
- **Compilatore**: il programma che traduce il codice in eseguibile. In Rust è molto severo e rifiuta di compilare codice che violerebbe le regole sulla memoria.
- **Move (spostamento)**: passare un valore a qualcun altro, perdendolo.
- **Borrow (prestito)**: dare a qualcun altro il permesso di guardare un valore, senza perderlo. Si scrive con `&`.
- **Clone**: copia esplicita di un valore, scritta `.clone()`.
- **`#[derive(Clone, Copy)]`**: `#[ ... ]` è un'annotazione sopra una definizione, come `@Override` in Java o `[Serializable]` in C#. `derive` significa "genera il codice per me" e tra parentesi c'è l'elenco delle capacità (in Rust: *trait*, simili alle interfacce di Java) che il tipo sotto ottiene senza scriverle a mano. `Clone` abilita `.clone()`. `Copy` fa sì che l'assegnazione copi invece di spostare.
- **`T` ed `E` (generici)**: segnaposto per un tipo, come in Java `List<T>`. In `Result<T, E>`, `T` è il tipo del valore di successo (dentro `Ok`) ed `E` quello dell'errore (dentro `Err`). In `Option<T>`, `T` è il tipo del valore dentro `Some`. Esempio: `Result<Tunnel, String>` ha `T = Tunnel` ed `E = String`.
- **Move parziale**: spostare un solo campo da una struct (per esempio `let name = raw_tunnel.name;`). Gli altri campi restano usabili, ma la struct intera no: non puoi più passarla a una funzione che la prende per valore, come `raw_tunnel.build()`. Per questo in `validate` si usa `.clone()` sul nome.
- **`&str` e `String`**: due tipi di testo. `&str` (`"ciao"`) è testo fisso in sola lettura, che si guarda soltanto. `String` è testo posseduto, che si può modificare e allungare. `String::from("ciao")`, `"ciao".to_string()` e `"ciao".into()` copiano un `&str` dentro una `String`.
- **`Tipo::funzione`**: il `::` accede a una funzione associata al tipo, come un metodo statico in Java o PHP. `String::from(...)` è la funzione `from` del tipo `String`.
- **Macro**: nome seguito da `!` (per esempio `vec![1, 2, 3]`, `format!`, `ensure!`). Non è una funzione normale: il compilatore la espande in altro codice prima di compilare. Serve, per esempio, quando gli argomenti sono in numero variabile.
- **`mut`**: parola chiave che rende modificabile una variabile (`let mut x`). Senza, in Rust non si può cambiare.

## 1. Ownership (proprietà della memoria)

Differenza di fondo rispetto a Java, C#, PHP:

- Nei linguaggi con GC, la memoria la libera il runtime mentre il programma gira.
- In Rust decide il compilatore prima, e rifiuta di compilare se le regole non tornano.

Tre regole:

1. Ogni valore ha **un solo proprietario**.
2. Assegnare o passare un valore a una funzione lo **sposta**: l'originale non è più usabile.
3. Per usarlo senza spostarlo si **presta** un riferimento: `&T` (sola lettura, anche in più punti) oppure `&mut T` (modifica, ma uno solo alla volta e nessun lettore nel frattempo).

Esempio minimo:

```rust
let a = String::from("ciao");
let b = a;          // il valore passa a b, a non è più valida
println!("{a}");    // errore di compilazione
```

In PHP o JS le due variabili punterebbero alla stessa stringa. In Rust no.

### Dove si vede in `src/config.rs`

- **Move**: `RawTunnel::build(self)` prende `self` per valore, quindi consuma il tunnel. Dopo la chiamata l'originale non esiste più.
- **Clone**: in `validate` si fa `raw_tunnel.name.clone()` prima di chiamare `build`, perché il nome serve ancora per il messaggio d'errore e `build` si porta via `raw_tunnel`.
- **Borrow**: `forbidden(&self.instance_connection_name, ...)` guarda il campo senza prenderlo. Per questo viene prima di `required(self.project, ...)`, che invece lo sposta.
- **Tipi `Copy`**: `Engine` e `Mode` hanno `derive(Clone, Copy)`. Sono piccoli e si duplicano da soli, quindi si possono usare più volte senza `clone()`. Una `String` no.
- **Parametri `&str` e `&Path`**: le funzioni che leggono soltanto prendono un riferimento, non vogliono possedere.

### Regola 3 nel dettaglio: `&` e `&mut`

È lo stesso principio di un lock lettori/scrittore (read/write lock), ma controllato dal compilatore prima che il programma parta:

- `&T`: sola lettura. Se ne possono avere quanti si vuole contemporaneamente.
- `&mut T`: lettura e modifica. Ce ne può essere **uno solo**, e mentre esiste nessun altro può guardare il valore.

Perché: se qualcuno modifica mentre un altro legge, il lettore può trovarsi con dati incoerenti o con un puntatore a memoria non più valida. Rust impedisce il caso a monte.

```rust
let mut v = vec![1, 2, 3];
let primo = &v[0];      // prestito in lettura, ancora vivo qui sotto
v.push(4);              // push modifica v (&mut): errore, c'è un lettore attivo
println!("{primo}");    // ultimo uso di primo
```

`push` può spostare l'array in un'altra zona di memoria per fare spazio, e `primo` punterebbe a memoria liberata. È il classico problema degli iteratori invalidati in C++. Rust si rifiuta di compilare. Per sistemare basta finire di usare `primo` prima del `push`.

In `src/config.rs`:

- `for t in &tunnels` presta in lettura `tunnels`, con molti `t` contemporanei: lecito.
- `tunnels.push(tunnel)` e `names.insert(...)` modificano la collezione. I metodi che modificano prendono `&mut self` in modo implicito, quindi non si vede il `&mut` nel punto di chiamata.
- `names` contiene riferimenti dentro `tunnels` (`t.name.as_str()`): finché `names` è in uso, `tunnels` non si può modificare né spostare.

### Esercizio

Togli `.clone()` dalla riga `let name = raw_tunnel.name.clone();` in `validate`, scrivendo `let name = raw_tunnel.name;`. Prima di compilare, scrivi dove pensi che si rompa e quale regola viene violata. Poi lancia `cargo build` e confronta con il messaggio del compilatore.

## 2. Tipi e capacità (`derive`, `Copy`, `Clone`)

Una **capacità** (in Rust: *trait*, simile a un'interfaccia di Java) è qualcosa che un tipo sa fare: essere copiato, stampato, confrontato con `==`, letto dal TOML. Le capacità appartengono al **tipo**, non alla variabile: `let n: u16 = 5` ha quelle di `u16`.

### Da dove arrivano le capacità

- **Tipi standard** (`u16`, `bool`, `String`, `Vec`): le hanno già, e sono fisse.
- **Tipi che scrivi tu** (`struct` ed `enum`): partono senza nessuna. Le aggiungi con `#[derive(...)]` sopra la definizione, e il compilatore scrive il codice al posto tuo.

`#[derive(Clone, Copy)]` si legge: "per il tipo qui sotto, genera la capacità di essere clonato e copiato". È un'annotazione, come `@Override` in Java. In Rust si chiama *attributo*.

### Tipi standard a confronto

| Tipo | `Copy` | `Clone` | `Debug` | `==` |
| --- | --- | --- | --- | --- |
| `u16`, `bool` (tipi semplici) | sì | sì | sì | sì |
| `String` | no | sì | sì | sì |
| `Vec<T>` | no | sì | sì | sì |

### Le capacità usate nel progetto

| Capacità | A cosa serve | Quando gira |
| --- | --- | --- |
| `Debug` | stampare il valore per capire cosa contiene | quando scrivi `{:?}` |
| `Clone` | duplicare il valore, contenuto compreso | quando scrivi `.clone()` |
| `Copy` | l'assegnazione copia invece di spostare | in automatico, a ogni `=` e a ogni passaggio a funzione |
| `PartialEq`, `Eq` | confrontare con `==` | quando scrivi `==` o `assert_eq!` |
| `Deserialize` | costruire il tipo da un file TOML | quando `toml::from_str` legge il file |

### Regole da ricordare

1. `derive` **dà** la capacità ma non la esegue. Resta ferma finché una riga di codice non la richiama. L'unica eccezione è `Copy`, che agisce da sola a ogni `=`.
2. Non esiste una scelta tra capacità. Ognuna ha il suo punto d'uso e non entra in competizione con le altre.
3. Regola di `=`: se il tipo è `Copy` copia, altrimenti sposta. Non c'è altro.
4. `Copy` si può dare solo se **tutti i campi** sono `Copy`. Un tipo con un campo `String` non può averlo.
5. `Copy` richiede anche `Clone`, quindi vanno scritti insieme. Su `Engine` il `Clone` non lo chiama nessuno: c'è solo perché `Copy` lo pretende.
6. `Debug` si mette quasi sempre. Le altre solo quando il codice ne ha bisogno.

### Perché `String` e `Vec` non sono `Copy`

Possiedono un blocco di memoria. Una copia automatica darebbe a due variabili lo stesso blocco, e alla fine lo libererebbero entrambe (doppia liberazione). Per questo si **spostano**. Se ne vuoi una copia vera scrivi `.clone()`, che duplica anche il contenuto.

### I tipi del progetto (`src/config.rs`)

| Tipo | Capacità | Perché |
| --- | --- | --- |
| `Engine`, `Mode` | `Debug, Clone, Copy, PartialEq, Eq, Deserialize` | piccoli, copiabili, letti dal TOML |
| `Settings` | `Debug, PartialEq, Eq, Deserialize` | letto dal TOML e confrontato nei test, ma contiene più campi e non serve copiarlo |
| `Tunnel`, `Config` | `Debug, PartialEq, Eq` | solo stampa e confronto nei test. Hanno campi `String`, quindi niente `Copy` |
| `RawConfig`, `RawTunnel` | `Debug, Deserialize` | forma grezza del TOML, letta dal file |

### `mut` e `&mut`: non sono la stessa cosa

- `let mut a`: lo scrivi tu. Dice "questa variabile si può modificare".
- `&mut a`: il prestito con permesso di modifica. Spesso non lo scrivi: lo crea Rust quando chiami un metodo che modifica, come `tunnels.push(x)`. Per questo nel progetto vedi `let mut`, ma mai la scritta `&mut`.

## 3. Assenza di valore ed errori: `Option`, `Result`, `?`

In Rust non esistono `null` né eccezioni. Una funzione che può non trovare un valore, o può fallire, lo dichiara **nel tipo che restituisce**. Il chiamante è obbligato dal compilatore a tenerne conto, quindi un `NullPointerException` non può succedere.

### `Option<T>`: "forse c'è un valore"

Ha due varianti:

- `Some(valore)`: il valore c'è
- `None`: non c'è

Sostituisce `null`. Esempio in `src/gcloud.rs`: `active_account()` restituisce `Option<String>`. `Some("mario@example.com")` se c'è un account loggato, `None` se non c'è nessuno.

### `Result<T, E>`: "è andata bene oppure no"

Ha due varianti:

- `Ok(valore)`: successo, con il risultato
- `Err(errore)`: fallimento, con la descrizione dell'errore

Sostituisce le eccezioni. Esempio: leggere un file può fallire, quindi `std::fs::read_to_string(path)` restituisce `Result<String, std::io::Error>`.

`Option` e `Result` sono degli `enum`: un tipo che vale una tra più varianti. Un `enum` può portarsi dietro un dato (qui il valore o l'errore).

### Come si usa il valore: `match`

`match` è come uno `switch` che ti obbliga a coprire tutti i casi:

```rust
match gcloud::active_account().await? {
    Some(acc) => println!("gcloud: loggato come {acc}"),
    None => anyhow::bail!("nessun account gcloud attivo"),
}
```

Se togli il ramo `None`, il compilatore non compila. Questo codice è in `src/main.rs`.

### L'operatore `?`

È la scorciatoia per propagare l'errore, come un `throw` automatico verso chi ha chiamato la funzione. Messo dopo un `Result`:

- se è `Ok(valore)`: estrae il valore e continua
- se è `Err(e)`: esce **subito** dalla funzione e restituisce quell'errore al chiamante

```rust
let text = std::fs::read_to_string(path)?;
```

Se `read_to_string` fallisce, la funzione che **contiene** questa riga (qui `load`) termina subito e restituisce l'errore a chi l'ha chiamata. Con più funzioni annidate l'errore risale un livello alla volta, perché ognuna usa il suo `?`:

```text
main()                          riceve l'errore da load e termina, exit code 1
  config::load(&path)?          riceve l'errore da read_to_string e lo passa a main
    std::fs::read_to_string(path)?   fallisce
```

La funzione che usa `?` deve a sua volta restituire un `Result`. È il motivo per cui `main`, `load`, `parse` e `active_account` restituiscono tutte un `Result`, ciascuna con un tipo diverso dentro le parentesi:

| Funzione | Firma | Valore in caso di successo |
| --- | --- | --- |
| `main` | `anyhow::Result<()>` | niente (`()`) |
| `load` | `Result<Config>` | un `Config` |
| `parse` | `Result<Config>` | un `Config` |
| `active_account` | `Result<Option<String>>` | un `Option<String>`: `Some(account)` oppure `None` |

Questi `Result` hanno un solo parametro perché vengono da `anyhow`. Vedi la sezione seguente.

### Perché `Result<Config>` ha un solo parametro

In realtà il nome `Result` può indicare due cose, e a decidere è la riga `use` in cima al file:

1. `Result<T, E>` di Rust, con due parametri: successo ed errore.
2. `Result<T>` di `anyhow`, una scorciatoia (come un `typedef` in C) in cui l'errore ha già un valore predefinito: `anyhow::Error`.

Le due scritture sono equivalenti:

```rust
Result<Config, anyhow::Error>   // esteso
Result<Config>                  // scorciatoia di anyhow
```

In `src/config.rs`, riga 5, c'è:

```rust
use anyhow::{Context, Result, ensure};
```

Significa: "in questo file, `Result` è quello di `anyhow`". Per questo quasi ovunque vedi `Result<Config>` con un solo parametro.

Il valore predefinito si può sostituire. Alla riga 154 `build` restituisce `Result<Tunnel, String>`: è sempre il `Result` di `anyhow`, ma con l'errore scelto a mano (una semplice `String`, perché la funzione si limita a descrivere cosa non va).

Perché si usa: `anyhow::Error` accetta qualunque tipo di errore. Scrivendo `Result<Config>` non devi dichiarare ogni volta quali errori possono uscire.

### `anyhow`: un tipo d'errore unico

`anyhow::Result<T>` significa `Result<T, anyhow::Error>`. `anyhow::Error` può contenere qualunque tipo d'errore, quindi non devi dichiarare tutti i tipi possibili:

- `.context("messaggio")` aggiunge una riga di spiegazione all'errore. Esempio in `config.rs`: `.context("TOML non valido")`.
- `anyhow::bail!("...")` esce subito con un errore. Equivale a `return Err(anyhow!("..."))`.
- `ensure!(condizione, "...")` fa `bail!` se la condizione è falsa.

### Altri pezzi che si vedono nel codice

- `Ok(())`: `()` è "niente" (come `void`). Significa "è andata bene e non c'è nulla da restituire".
- `.ok_or("messaggio")`: trasforma un `Option` in un `Result`. `None` diventa `Err("messaggio")`. In `config.rs` serve per il campo `mode` mancante.
- `.map_err(|e| ...)`: trasforma il tipo dell'errore, lasciando intatto il successo.
- `unwrap()` e `expect()`: estraggono il valore ma **bloccano il programma** se è `None` o `Err`. Nel progetto sono vietati fuori dai test (regola in `CLAUDE.md`).

### Riassunto

| Serve per | Tipo | Varianti | Gestione |
| --- | --- | --- | --- |
| Valore che può mancare | `Option<T>` | `Some(v)` / `None` | `match`, `ok_or` |
| Operazione che può fallire | `Result<T, E>` | `Ok(v)` / `Err(e)` | `match`, `?` |

## Prossimi argomenti

- Async e `tokio` (task, `select!`, cancellazione)
- Trait e `derive` (cosa generano le macro come `#[derive(Deserialize)]`)
- Mappa del programma: chi avvia cosa e come passa un tunnel da `Down` a `Up`
