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

Se reportan p50/p95/p99 en nanosegundos y operaciones por segundo. Para los casos
más cortos, `operations_per_sample` agrupa varias operaciones para reducir el
error del reloj: sus percentiles corresponden a medias por lote, no a la latencia
de cada operación individual. El throughput incluye el bucle de medición y la
recolección de muestras. No es una medida de capacidad HTTP ni de trabajo real.

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
