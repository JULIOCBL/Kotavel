# Pruebas de las herramientas

Desde la raíz de Kotavel:

```sh
python3 -m unittest discover -s tests -p "test_*.py"
```

Verifican perfiles y preparadores de Windows/Linux/macOS, reutilización de
herramientas, ejecución configurable, despacho de comandos a un consumidor,
dependencias duplicadas, recuperación ante errores y bloqueo entre ejecuciones.
Utilizan fixtures temporales y simulaciones; no descargan paquetes.
Algunas pruebas POSIX se omiten en Windows.

Las pruebas de la entrada del consumidor y de su switch de framework permanecen
en Laboratory-Kotavel. La validación nativa de Windows y Linux necesita ejecutar
allí el flujo de preparación y compilación.

## Núcleo C++

`kotavel.messages` verifica sustitución de parámetros, valores literales,
comandos Windows/Unix y errores de claves o parámetros. `kotavel.console`
también comprueba el diagnóstico completo cuando no existe el archivo de entorno.

Desde Kotavel, con CMake y un compilador C++23 disponibles:

```sh
cmake -S . -B build/core -DCMAKE_BUILD_TYPE=Debug -DKOTAVEL_BUILD_TESTS=ON
cmake --build build/core --config Debug
ctest --test-dir build/core -C Debug --output-on-failure
```

Se comprueban recuperación seguida de otra operación, liberación de recursos,
rechazo de ejecución en estados inválidos, fallo de arranque, retención de
registros y salida de emergencia. El Laboratorio prueba el enlace y una
operación posterior a un fallo desde un consumidor real.

Las pruebas de diagnóstico verifican archivo y línea de construcción, ocultación
de detalles en producción, presentación de desarrollo, reglas por código,
clasificación de excepciones externas, ausencia de estado entre operaciones y
contratos reservados que devuelven `not_implemented`. Las pruebas del logger cubren
los nueve niveles y cada umbral. Las trazas pueden quedar explícitamente no
disponibles en un toolchain sin soporte; las pruebas no afirman que se resuelvan
símbolos en todas las plataformas.

Las pruebas de logging añaden contexto, causas anidadas acotadas, fallo observable
del registro, límites por ventana, escritura desde cuatro hilos, aislamiento de
trazas por hilo y rechazo de carpetas compartidas. Python 3 ejecuta también una
prueba con dos procesos reales y comprueba que el bloqueo se libera al cerrar.
Las pruebas del núcleo requieren ahora Python 3, además de CMake y C++23.

Desde un consumidor configurado en modo local, `python3 kotavel.py test --framework`
ejecuta una comprobación completa: pruebas Python del framework y del consumidor,
compilación y pruebas del núcleo, y compilación y pruebas de integración del
consumidor con Conan. Se detiene al primer fallo. Requiere las suites
`tests/test_*.py` en ambos proyectos. Usa las herramientas locales y el compilador
seleccionado del consumidor; guarda el núcleo en `build/framework-tests/<Debug|Release>`
y el consumidor en `build/<Debug|Release>`. `--build-type Release` cambia el tipo
de compilación; `test` sin `--framework` conserva la ejecución de CTest existente.

`.github/workflows/core.yml` prepara Debug/Release en Windows, Linux y macOS,
además de ASan/UBSan y TSan en trabajos separados de Linux. La matriz no demuestra
compatibilidad hasta que termine correctamente en GitHub. En Windows esa matriz
usa MSVC; el toolchain LLVM-MinGW del preparador aún necesita su propia validación.

`kotavel.config` comprueba el mapa plano de configuración en memoria: inserción,
actualización, tipos, búsquedas con vistas, estabilidad de referencias al insertar
y errores sin valores sensibles. No modifica archivos de configuración.

`kotavel.config.extended` comprueba grupos completos, listas de escalares y mapas,
null explícito, lectura múltiple, lotes atómicos y coherencia al sustituir grupos.

`kotavel.config.edge` verifica límites de profundidad, longitud y cantidad de
nodos; grupos con prefijos parecidos; colisiones y registros inválidos; rollback
de merges y lotes fallidos; claves repetidas; y restauración del helper en scopes
anidados. Para ejecutar únicamente las tres suites de configuración:

```sh
cmake --build build/core --config Debug \
  --target kotavel_config_tests kotavel_config_extended_tests kotavel_config_edge_tests
ctest --test-dir build/core -C Debug -R '^kotavel\.config' --output-on-failure
```

Para ejecutar solo la suite de límites y errores:

```sh
ctest --test-dir build/core -C Debug -R '^kotavel\.config\.edge$' --output-on-failure
```

CTest usa los nombres registrados en CMake; las carpetas organizan los fuentes,
no cambian la forma de invocar cada suite.

Las fuentes unitarias se organizan por módulo en `tests/unit/config/`,
`tests/unit/env/`, `tests/unit/console/`, `tests/unit/metrics/`, `tests/unit/core/`,
`tests/unit/system/`, `tests/unit/diagnostics/` y `tests/unit/logging/`. Las pruebas entre procesos
permanecen en `tests/integration/`.

`kotavel.console.logging` verifica el adaptador Config → LogOptions: defaults,
canales single/stderr/null, tipos y límites inválidos, rutas relativas/absolutas,
ausencia de archivos con stderr/null y conservación de errores con logging apagado.
`kotavel.metrics` comprueba muestras reales sin fijar valores del sistema operativo,
cálculo de CPU entre lecturas, límites del intervalo, rechazo de callbacks inválidos,
continuidad tras una excepción, rechazo de un segundo arranque, parada idempotente,
reinicio y parada solicitada desde el callback. Verifica también que no se invoque
el callback después de que `stop()` regrese. La matriz nativa de CI compila y ejecuta
estas mismas pruebas en Linux, macOS y Windows.
`kotavel.metrics.stream` conecta un cliente local real y comprueba los eventos de
arranque y muestra, el enlace a loopback y la eliminación del descriptor al cerrar.
