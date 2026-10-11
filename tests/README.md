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

Desde un consumidor configurado en modo local también puedes compilar y probar
el núcleo con `python3 kotavel.py test --framework`. Usa el perfil del consumidor
y guarda la compilación en su carpeta `build/framework-tests/<Debug|Release>`.

`.github/workflows/core.yml` prepara Debug/Release en Windows, Linux y macOS,
además de ASan/UBSan y TSan en trabajos separados de Linux. La matriz no demuestra
compatibilidad hasta que termine correctamente en GitHub. En Windows esa matriz
usa MSVC; el toolchain LLVM-MinGW del preparador aún necesita su propia validación.
