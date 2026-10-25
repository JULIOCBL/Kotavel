# Red

`network` define la consulta de direcciones de red y exporta el proveedor nativo. La API permite a la aplicación mostrar o reportar interfaces utilizables sin mezclar esa lógica con el núcleo.

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

## API

`NetworkAddressesProvider::local_ipv4_addresses()` devuelve las IPv4 de
interfaces activas no loopback. `NativeNetworkAddressesProvider` implementa el
trait para el sistema actual; filtra direcciones no especificadas, loopback y
link-local, y elimina duplicados. La operación devuelve `io::Result<Vec<String>>`.
