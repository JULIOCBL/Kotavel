# Flujo HTTP y medición de rendimiento

Revisión del 23 de octubre de 2026 sobre el árbol de trabajo existente. El
objetivo es reducir trabajo repetido conservando las respuestas, el orden de
middleware, las validaciones, el aislamiento de sesiones y el log por petición.

## Recorrido revisado

1. **Transporte y rutas:** Drogon recibe y analiza HTTP. El control previo de
   métodos consulta un índice inmutable de rutas literales construido al iniciar.
   Solo los patrones parametrizados requieren comparar segmentos. Drogon sigue
   resolviendo la ruta final y su precedencia entre rutas estáticas y dinámicas.
2. **Request:** el adaptador copia los datos una vez para el handler. JSON,
   formularios y archivos mantienen sus reglas de análisis y normalización.
   Las consultas internas con `string_view` evitan crear claves temporales;
   `filled()` comprueba los contenedores sin copiar sus elementos.
3. **Kernel:** se negocia `Accept`, se comprueba la entrada y se ejecutan los
   middleware globales, del grupo y de la ruta. Las continuaciones ahora capturan
   un puntero y un índice para reducir asignaciones en `std::function`.
4. **FormRequest:** se conservan autorización, reglas, `bail`, `nullable`, errores
   acumulados y campos permitidos. Los campos compuestos reutilizan una sola
   colección de entrada y buscan sus claves por prefijo, evitando copiar y
   recorrer toda la petición por cada objeto validado.
5. **Sesiones:** las opciones fijas de la cookie se preparan al crear el
   middleware; cada petición conserva solo la duración que necesita su estado.
   El contador y la exclusión de barridos pertenecen a esa instancia de
   middleware y sus copias. Si un barrido sigue ejecutándose, otros trabajadores
   continúan sin iniciar otro barrido redundante. Un fallo libera esa exclusión.
6. **Respuesta:** `ApiResponse` escribe el sobre directamente con RapidJSON,
   conservando campos, orden, escape y códigos. Evita construir mapas JSON
   intermedios para cada error o para los metadatos.
7. **Salida y consola:** el adaptador entrega la respuesta y registra cada
   acceso utilizando método, ruta, estado y tiempos. El log ya no reconstruye
   una segunda `Request` ni vuelve a analizar JSON o uploads. Las respuestas
   404/405 solo necesitan metadatos y `Accept`; tampoco analizan el cuerpo.

## Comparación exploratoria

Datos completos: [http-flow-2026-10-23.json](../../benchmarks/baselines/http-flow-2026-10-23.json).

Mac ARM64, Apple Clang 15, núcleo Release (`-O3 -DNDEBUG`), sin sanitizers.
Las dependencias Conan de transporte se reutilizaron en Debug, iguales antes
y después. Estas cifras no representan capacidad de producción ni una
comparación entre frameworks.

Microbenchmarks: mediana de los p50 de tres ejecuciones por versión.

| Escenario | Antes | Después | Reducción del tiempo |
| --- | ---: | ---: | ---: |
| Analizar JSON anidado | 27,00 µs | 25,58 µs | 5,2 % |
| Consultar una clave larga | 99,17 ns | 44,58 ns | 55,0 % |
| Validar 40 objetos anidados | 343,08 µs | 42,46 µs | 87,6 % |
| Serializar 40 errores | 11,88 µs | 7,92 µs | 33,3 % |
| Kernel con cinco middleware | 491,60 ns | 270,80 ns | 44,9 % |

HTTP real: un cliente keep-alive en loopback, logs activos redirigidos a archivo,
10 solicitudes de calentamiento y 100 muestras por escenario y versión.

| Escenario | p50 antes / después | p95 antes / después | p99 antes / después |
| --- | ---: | ---: | ---: |
| GET con 1.000 rutas | 0,184 / 0,094 ms | 0,215 / 0,138 ms | 0,240 / 0,169 ms |
| POST JSON de aproximadamente 100 KB | 2,713 / 2,163 ms | 4,177 / 2,383 ms | 8,782 / 2,573 ms |
| Contador de sesión en archivo | 0,367 / 0,312 ms | 0,410 / 0,407 ms | 0,435 / 0,454 ms |

La cola de latencia de sesiones no mejoró uniformemente: su p99 aumentó en esta
muestra. La E/S, los bloqueos y el barrido siguen influyendo. Los resultados de
una ejecución corta no establecen límites de latencia.

## Reproducir y verificar

Activar `KOTAVEL_BUILD_BENCHMARKS=ON` en un build Release con las dependencias
resueltas. `kotavel_http_benchmark` se puede ejecutar con `benchmarks/run.py`,
igual que el benchmark del núcleo. Con `KOTAVEL_BUILD_HTTP=ON` y
`KOTAVEL_BUILD_TESTS=ON`, CTest también incluye `kotavel.http.transport`.

```sh
python3 benchmarks/run.py build/http-performance/kotavel_http_benchmark --output build/http-performance/measurement.json
python3 tests/integration/http_transport.py build/http-performance/kotavel_http_transport_fixture --measure build/http-performance/transport.json
```

La integración arranca un servidor propio en un puerto temporal, comprueba
éxitos, HEAD, 400, 403, 404, 405, 406, rutas parametrizadas, JSON grande,
persistencia de cookies y una línea de log por respuesta. Siempre detiene ese
servidor al terminar. Las pruebas de sesiones incluyen barridos concurrentes,
instancias independientes y recuperación después de errores de mantenimiento.

## Costes que permanecen

- El driver distribuye sesiones por prefijo y limpia incrementalmente hasta
  128 archivos por llamada. Un ciclo completo aún recorre todos los registros;
  la espera por bloqueos y la E/S no tienen un límite temporal estricto.
  Las mediciones anteriores a esta distribución no cuantifican su mejora.
- Guardar una sesión sigue realizando E/S y renovación de su vencimiento en
  cada petición. Se conserva la escritura atómica y el bloqueo durante la
  actualización para proteger la persistencia y los incrementos concurrentes.
- `FormRequest` conserva su copia propietaria de `Request`; cambiar ese modelo
  requiere revisar el ciclo de vida y las extensiones de los consumidores.
- Las expresiones regulares de validación se compilan al validar. Un caché
  necesitaría límites de memoria y claves que incluyan las opciones de la regla.
- La consola mantiene escritura sincronizada y flush por respuesta. Una terminal
  lenta puede limitar el rendimiento. No se desactivaron logs para mejorar las
  cifras.
