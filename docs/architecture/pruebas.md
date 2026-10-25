# Pruebas del framework

Desde Laboratory-Kotavel, el comando canónico es:

```sh
cargo kotavel test --framework
```

Ejecuta las pruebas de todos los crates del workspace Kotavel y después las del
Laboratorio. Las pruebas unitarias viven junto a cada módulo; las pruebas de
ejecución del CLI están en `crates/core/tests/`.

Al añadir pruebas al núcleo o al Laboratorio, comprueba que el comando anterior
las incluya. Mantén las pruebas de contratos portables y las de adaptadores en
el target/plataforma correspondiente.
