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
