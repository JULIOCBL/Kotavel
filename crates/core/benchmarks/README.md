# Medición exploratoria del núcleo

La revisión y las mediciones del recorrido HTTP están documentadas en
[Flujo HTTP y rendimiento](../docs/http/performance.md). El target
`kotavel_http_benchmark` utiliza el mismo ejecutor `benchmarks/run.py`.

Desde Kotavel, compila sin sanitizers y ejecuta tres repeticiones:

```sh
cmake -S . -B build/core-release -DCMAKE_BUILD_TYPE=Release -DKOTAVEL_BUILD_BENCHMARKS=ON
cmake --build build/core-release --config Release
python3 benchmarks/run.py build/core-release/kotavel_core_benchmark --output build/benchmark-results.json
```

Con un generador de varias configuraciones, como Visual Studio, el ejecutable
está en `build/core-release/Release/kotavel_core_benchmark.exe`.

El programa calienta cada escenario antes de medir y termina con error si una
operación devuelve un resultado inesperado. Mide operación vacía exitosa, error
recuperable filtrado con/sin captura de pila, escritura a archivo y reporte
limitado. La consola del logger está desactivada. Las carpetas temporales se
eliminan al terminar.

## Sesiones SQLite bajo concurrencia

El target opcional `kotavel_session_benchmark` genera una carga contra el
middleware de Kotavel y SQLite WAL. Por defecto usa 500 clientes durante cinco
segundos, con dos solicitudes por segundo por cliente y una escritura cada cinco
solicitudes. Se puede pasar otro número de clientes y duración en segundos:

```sh
cmake -S . -B build/session-release -DCMAKE_BUILD_TYPE=Release -DKOTAVEL_BUILD_BENCHMARKS=ON
cmake --build build/session-release --target kotavel_session_benchmark
build/session-release/kotavel_session_benchmark
build/session-release/kotavel_session_benchmark 100 3
build/session-release/kotavel_session_benchmark 500 5 shared
build/session-release/kotavel_session_benchmark 500 5 shared-keys
build/session-release/kotavel_session_benchmark 500 5 independent 0
```

Para repetir los tres perfiles, verificar carga y escrituras durables en cada
corrida y guardar JSON con el hash de las fuentes:

```sh
python3 benchmarks/run_session_benchmark.py \
  build/session-release/kotavel_session_benchmark \
  --clients 500 --seconds 10 --repeats 3 --build "Release macOS ARM64" \
  --output build/session-acceptance.json
```

El runner falla si la carga ofrecida, la cantidad de solicitudes, las escrituras
durables, los conflictos o las respuestas difieren del escenario configurado.
El JSON conserva las mediciones de cada repetición y resume mediana, mínimo y
máximo; no mezcla resultados de perfiles ni de máquinas distintas.

El tercer argumento acepta `independent` (predeterminado), `shared` o
`shared-keys`. El primero asigna una sesión distinta a cada cliente; `shared`
hace que todos compitan por incrementar la misma clave de una sesión; y
`shared-keys` hace que todos usen la misma sesión con una clave independiente por
cliente. Las fases se desplazan entre clientes para distribuir en el tiempo el
20% de escrituras, en vez de sincronizarlas. Ambos casos de sesión compartida usan `Session::increment()`, que permite
rebasar actualizaciones atómicas de contadores. Cualquier 409 se reporta y hace
fallar el benchmark. El cuarto argumento opcional ajusta la ventana de
agrupamiento en microsegundos (0–10000); el default es 0. La comprobación final
exige que la suma durable coincida con las escrituras aceptadas.

La salida CSV incluye carga (independiente/compartida), clientes, sesiones,
solicitudes completadas, escrituras confirmadas, conflictos, fallos, percentiles
de latencia general y de escrituras, throughput, hit rate de caché, número de
adquisiciones contenciosas de locks de shard, espera media/máxima de esas
adquisiciones y bytes residentes estimados, CPU media y máxima del proceso, RSS
máxima muestreada, espera por capacidad de cola, tiempo en cola del escritor,
duración de ejecución de escrituras, profundidad máxima, número de transacciones
de lote, escrituras incluidas y tamaño medio/máximo del lote. La comprobación final lee directamente desde SQLite,
evitando que una entrada caliente de la caché oculte una escritura no durable;
además comprueba el contador esperado por sesión. Mide
el middleware directamente, sin sockets ni Drogon; sus resultados sirven para
comparar cambios del driver SQLite bajo una carga repetible, no para afirmar
capacidad HTTP. La comparación con la sesión nativa usa el fixture de transporte
descrito abajo.

Las latencias de sesión se reportan en milisegundos (`*_ms`) y el throughput en
solicitudes por segundo. La CPU se expresa como porcentaje de un núcleo (puede
superar 100% en varios núcleos); RSS se muestrea cada 100 ms mediante el
adaptador nativo de métricas. El muestreador consume recursos y forma parte de
la medición. Esta carga mide el middleware directamente, sin sockets; no es una
medida de capacidad HTTP.

La prueba del transporte HTTP también compara 64 clientes simultáneos entre el
middleware Kotavel y una ruta con `drogon::Session::modify()`:

```sh
python3 tests/integration/http_transport.py \
  build/session-http/kotavel_http_transport_fixture \
  --measure build/session-http/session-comparison.json
```

Ambas rutas usan el mismo listener loopback de Drogon, 16 hilos del servidor y
64 conexiones cliente. El dato registrado en JSON es throughput de esa ráfaga.
La sesión nativa de Drogon solo vive en memoria; Kotavel confirma cada escritura
en SQLite WAL con `synchronous=FULL`. La cifra sirve para ver el costo observado
de persistencia durable en este equipo, no es una comparación equivalente ni un
umbral de rendimiento.

La medición actual de tres ejecuciones y su hash de fuentes están en
[session-http-2026-10-24.json](baselines/session-http-2026-10-24.json). La mediana
de esta ráfaga fue 6.816 req/s para Kotavel y 9.515 req/s para Drogon nativo.
La referencia del 23 de octubre conserva los resultados de la revisión anterior;
ambas diferencias deben leerse junto con sus garantías distintas de persistencia.

Para medir sesiones sobre sockets HTTP reales con la carga de aceptación (500
clientes persistentes, dos solicitudes/s por cliente y 20% de escrituras):

```sh
cmake -S . -B build/session-http \
  -DCMAKE_BUILD_TYPE=Debug -DKOTAVEL_BUILD_HTTP=ON \
  -DKOTAVEL_BUILD_TESTS=ON -DKOTAVEL_BUILD_BENCHMARKS=ON
cmake --build build/session-http --target kotavel_http_transport_fixture --config Debug
python3 benchmarks/http_session_load.py \
  build/session-http/kotavel_http_transport_fixture --clients 500 --seconds 10 \
  --build "Debug macOS ARM64" --output build/session-http/load-independent.json
python3 benchmarks/http_session_load.py \
  build/session-http/kotavel_http_transport_fixture --clients 500 --seconds 10 \
  --workload shared --build "Debug macOS ARM64" \
  --output build/session-http/load-shared.json
python3 benchmarks/http_session_load.py \
  build/session-http/kotavel_http_transport_fixture --clients 500 --seconds 10 \
  --instances 2 --workload shared --build "Debug macOS ARM64" \
  --output build/session-http/load-shared-2-processes.json
python3 benchmarks/http_session_load.py \
  build/http-release/kotavel_http_transport_fixture --clients 500 --seconds 10 \
  --instances 2 --workload shared --build "Release macOS ARM64" \
  --output build/http-release/load-shared-2-processes.json
```

El primer perfil crea una sesión independiente por cliente; el segundo hace que
los 500 clientes usen el mismo ID de sesión. El tercero ejecuta dos procesos
Drogon con cachés independientes sobre el mismo archivo SQLite. Todos exigen la
carga completa, verifican las escrituras y comprueban los contadores después de
matar y reiniciar el servidor o los procesos con la misma base SQLite. El script
informa throughput y p50/p95/p99 de extremo a extremo. La medición de 500 clientes
es optativa; `ctest` ejecuta una prueba más corta de dos procesos para comprobar
la consistencia entre cachés en cada plataforma. Cada JSON registra plataforma,
perfil de compilación, hash de fuentes, carga ofrecida, solicitudes, escrituras
confirmadas y recuperación durable.
Las corridas actuales están en los reportes de [sesiones independientes](baselines/session-http-load-independent-2026-10-24.json)
y [una sesión compartida](baselines/session-http-load-shared-2026-10-24.json): ambas
completaron 10.000 solicitudes (997,7 req/s), 2.000 escrituras durables y el
reinicio, sin fallos; el p95 fue 1,42 ms y 1,39 ms, respectivamente.
La repetición Release con el mismo listener, carga y hardware está en los
reportes de [sesiones independientes](baselines/session-http-load-independent-release-2026-10-24.json)
y [una sesión compartida](baselines/session-http-load-shared-release-2026-10-24.json):
completaron la misma carga a 997,6 y 997,4 req/s, con p95 de 1,49 y 1,45 ms.
El build Release con Drogon también pasó las 31 pruebas CTest.
La corrida Release con dos procesos está en
[session-http-load-shared-2-process-release-2026-10-24.json](baselines/session-http-load-shared-2-process-release-2026-10-24.json):
completó 10.000 solicitudes y 2.000 escrituras durables a 997,6 req/s, con p95
de 1,99 ms y cero fallos después de reiniciar ambos servidores.

La comparación con el backend de archivos retirado está en
[session-file-vs-sqlite-2026-10-24.json](baselines/session-file-vs-sqlite-2026-10-24.json).
Se reconstruyó desde el commit `ce5caeb`, con la fixture histórica ajustada a 16
hilos para igualar la configuración de SQLite. Tres parejas alternadas usaron
64 clientes simultáneos que incrementan una sesión compartida y verificaron el
contador después de matar y reiniciar el servidor. La mediana fue 3.637 req/s y
9,84 ms de p95 para archivos, frente a 6.710 req/s y 3,20 ms para SQLite WAL.
SQLite fue 1,85 veces más rápido en esta carga; el p95 bajó 67%. La persistencia
no ofrece garantías idénticas: el backend anterior reemplazaba archivos y SQLite
espera un commit con `synchronous=FULL`.

El script reproducible acepta ambos ejecutables como argumentos:
[`session_transport_compare.py`](session_transport_compare.py). El ejecutable
FileSessionStore solo se construye desde la revisión histórica; no forma parte
del código actual.

La referencia de concurrencia repite cada perfil tres veces durante dos
segundos: [session-concurrency-2026-10-23.json](baselines/session-concurrency-2026-10-23.json).
Con ventana de lote en cero, las medianas fueron:

| Perfil | Clientes/sesiones | Throughput | p95 | Lote medio | Locks contenciosos (mediana) | Espera media / máxima | CPU media | RSS pico |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| independent | 500 / 500 | 997 req/s | 0,751 ms | 1,02 | 1 | 13,7 / 13,7 µs | 17,6% | 15,0 MiB |
| shared-keys | 500 / 1 | 996 req/s | 3,104 ms | 1,02 | 105 | 21,7 / 168,1 µs | 107,3% | 16,3 MiB |
| shared | 1.000 / 1 | 1.991 req/s | 0,726 ms | 1,15 | 552 | 42,8 / 622,0 µs | 33,1% | 24,0 MiB |

Los tres perfiles dieron cero conflictos y fallos; la verificación durable
coincidió con las escrituras aceptadas. Casi todos los lotes tienen una sola
operación con esa configuración. La prueba unitaria abre una ventana controlada
y confirma que 96 commits independientes sí se agrupen en transacciones
durables.

La [corrida de resistencia de 10 segundos](baselines/session-soak-2026-10-23.json)
completó 10.000 solicitudes en cada perfil de 500 clientes y 20.000 en el
contador de 1.000 clientes, con cero conflictos/fallos y conteos durables
correctos. Sus p95 fueron 0,781 ms, 6,914 ms y 0,837 ms, respectivamente. Las
corridas describen esta máquina con build Debug; no son un límite de producción.

La [corrida equivalente en Release](baselines/session-soak-release-2026-10-23.json)
también completó los tres perfiles sin conflictos ni fallos. Sus p95 fueron
0,648 ms con sesiones independientes, 1,580 ms con claves distintas en una
sesión y 0,630 ms con un contador compartido. La CPU media fue 12,7%, 48,2% y
20,4%, respectivamente; el RSS máximo observado fue 24,6 MiB.

La [validación de aceptación repetida en Release](baselines/session-acceptance-2026-10-24.json)
ejecutó tres veces cada perfil solicitado por el análisis: 500 clientes, 1.000
solicitudes/s ofrecidas y 20% de escrituras durante 10 segundos por corrida.
Las nueve corridas completaron las 10.000 solicitudes y confirmaron 2.000
escrituras en SQLite, sin conflictos ni errores. Las medianas de throughput
fueron 999 req/s en los tres perfiles; el p95 fue 0,914 ms con sesiones
independientes, 0,902 ms con un contador compartido y 1,465 ms al modificar
claves distintas de una sesión. El payload durable mediano fue 62 bytes por
sesión independiente, 62 bytes para el contador compartido y 20,9 KiB para la
sesión con claves distintas. El RSS pico mediano fue 14,8, 14,2 y 16,7 MiB,
respectivamente. El artefacto registra cada corrida, CPU, contención, latencia
del escritor, memoria y hash de fuentes. Se generó con
[`run_session_benchmark.py`](run_session_benchmark.py), que verifica la carga y
las escrituras durables además de guardar mediana, mínimo y máximo.
El payload durable por sesión se informa por separado del RSS máximo y de la
ocupación de la caché; se deriva del tamaño estimado guardado en cada registro.

La [prueba de escala con 2.000 clientes](baselines/session-scale-2026-10-24.json)
ejecutó una corrida de 10 segundos por perfil a 4.000 solicitudes/s ofrecidas.
Las tres corridas secuenciales completaron 120.000 solicitudes y 24.000
escrituras sin conflictos ni errores; cada contador durable coincidió con las
escrituras aceptadas. En la corrida actual, el p95 fue 0,924 ms para 2.000
sesiones, 1,137 ms para un contador compartido y 3,278 ms al mezclar claves
independientes dentro de una sesión. Frente a la
[referencia previa al seguimiento de claves modificadas](baselines/session-scale-before-delta-2026-10-24.json),
ese último perfil redujo el p95 67%, la CPU media de 520% a 248% y el RSS pico
de 121 a 61 MiB. La caché entrega snapshots inmutables compartidos y cada
petición conserva solo su overlay de claves; el mapa completo se materializa al
persistir o solicitar `Session::all()`. `resident_bytes` es la ocupación al final
de la corrida, no el máximo: las escrituras invalidan la entrada compartida.

La matriz `native` de `.github/workflows/core.yml` habilita benchmarks en
Ubuntu, macOS y Windows, con el adaptador Drogon habilitado. En Debug y Release,
`ctest` ejecuta las pruebas de transporte HTTP, una integración que inicia dos
procesos Drogon sobre la misma base SQLite y tres cargas representativas: 500
sesiones independientes, 500 clientes que actualizan claves independientes de
una sesión y 1.000 clientes sobre un contador compartido. Cada carga valida la
persistencia durable y falla ante conflictos o errores, además de compilar los
adaptadores de métricas.

## Primera referencia exploratoria

[macos-initial.json](baselines/macos-initial.json) conserva tres ejecuciones del
10 de octubre de 2026, con hash SHA-256 de fuentes, cabeceras, benchmark y CMake:

- macOS 26.3.1, ARM64; modelo de CPU y memoria física no disponibles en esta sesión.
- Apple Clang 15.0.0 (clang-1500.3.9.4), libc++, CMake 4.3.
- Release, `-O3 -DNDEBUG -std=c++2b`, sin LTO ni sanitizers.
- Un hilo, E/S a almacenamiento temporal local, 100 operaciones de calentamiento.
- 100 000 operaciones exitosas; 5 000 errores por variante; 2 000 reportes a archivo;
  100 000 reportes limitados por ejecución.

Las primeras mediciones muestran variación entre repeticiones, especialmente en
E/S y colas de latencia. No permiten afirmar una ventaja frente a Laravel ni
establecer un presupuesto de producción. Esta referencia aún requiere hardware
identificado, consumo de CPU/memoria, carga representativa y pruebas prolongadas.
No debe utilizarse como umbral automático de regresión.

La captura configurada se prueba por separado de la escritura: comparar errores
filtrados con escritura de logs mide trabajos distintos. No se ha introducido
logging asíncrono ni un pool de memoria a partir de estas cifras.
