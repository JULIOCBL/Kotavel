# Red

`network` define la consulta de direcciones de red y la lista ordenada de puertos
HTTP de Kotavel.

## Ejemplo

```rust
use kotavel::network::{NativeNetworkAddressesProvider, NetworkAddressesProvider};

let provider = NativeNetworkAddressesProvider::new();
let addresses = provider.local_ipv4_addresses()?;
for address in addresses {
    println!("{address}");
}
```

El adaptador filtra direcciones no utilizables y elimina duplicados. Si una aplicación necesita una política distinta, puede implementar el contrato en lugar de cambiar el comportamiento compartido.

## Puertos candidatos HTTP

`DISCOVERY_PORT_CANDIDATES` expone, en orden, los puertos que intenta el
servidor HTTP cuando no se define `HTTP_PORT`:

```rust
use kotavel::network::DISCOVERY_PORT_CANDIDATES;

assert_eq!(DISCOVERY_PORT_CANDIDATES[0], 3000);
```

Los candidatos son `3000`, `5000`, `8000`, `8080`, `9000`, `28474`, `28475`,
`28476`, `28477` y `28478`. El servidor intenta enlazarse directamente a cada
uno y usa el primero disponible. Un `HTTP_PORT` explícito no cambia de puerto
si está ocupado; el inicio informa el error.

## API

`NetworkAddressesProvider::local_ipv4_addresses()` devuelve las IPv4 de
interfaces activas no loopback. `NativeNetworkAddressesProvider` implementa el
trait para el sistema actual; filtra direcciones no especificadas, loopback y
link-local, y elimina duplicados. La operación devuelve `io::Result<Vec<String>>`.
