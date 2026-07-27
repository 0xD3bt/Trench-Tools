# Trench.Tools - El stack de trading de código abierto para traders de Solana.

<p align="center">
  <img src="assets/trench-tools-hero.png" alt="Trench Tools - the open-source trading stack for the trenches" width="100%">
</p>

<div align="center">
  <table>
    <tr>
      <td align="center"><a href="https://trench.tools/"><strong>Sitio Web</strong></a></td>
      <td align="center"><a href="https://x.com/TrenchDotTools"><strong>@TrenchDotTools</strong></a></td>
      <td align="center"><a href="https://x.com/0xd3bt"><strong>@0xd3bt</strong></a></td>
      <td align="center"><a href="https://x.com/i/communities/2038790841418838419"><strong>Comunidad</strong></a></td>
    </tr>
  </table>
</div>

<div align="center">
  <table>
    <tr>
      <td align="center">
        <strong>Dirección del Contrato</strong><br>
        <code>L73w5odyo5ZdJ1fPp319nfjqaFfHDdKifRmM8Kxpump</code>
      </td>
    </tr>
  </table>
</div>

Trench Tools es un stack de ejecución auto-alojado y de código abierto para flujos de trabajo de trading y lanzamientos en Solana.

El stack está construido alrededor de un motor de ejecución local en Rust, una extensión de navegador para plataformas compatibles y LaunchDeck para operaciones de launchpads. El runtime mantiene las billeteras, presets, configuración de RPC, construcción de transacciones, firma y política de envío en infraestructura que tú controlas.

Utilízalo localmente para configuración y pruebas. Para obtener una menor latencia y un límite de seguridad más limpio en trading real, ejecútalo en un VPS privado económico cerca de tus endpoints de RPC y de proveedor de ejecución más cercanos.

El proyecto se encuentra en desarrollo activo. Asegúrate de que tu configuración esté configurada y verificada de extremo a extremo antes de utilizarlo para trading real.

## Cómo Funciona

Trench Tools separa la interfaz del navegador de la ejecución:

- Ejecuta `execution-engine` y `LaunchDeck` en tu propia máquina o en un VPS privado. Para trading en vivo, la configuración recomendada es un VPS económico cerca de tus endpoints de RPC y proveedor de ejecución.
- Instala la extensión de Chrome/Edge en tu navegador local. Esta inyecta los controles de Trench Tools en plataformas compatibles como Axiom, J7Tracker y X.
- Cuando operas desde una plataforma compatible, la extensión envía la intención de trade a tu propio motor de ejecución. El motor se encarga de la validación de la ruta, la construcción/firma/envío de la transacción, las confirmaciones y los eventos de PnL utilizando tus billeteras, presets, RPCs y proveedores configurados.
- Si el runtime está en un VPS, mantén los puertos raw privados y conecta tu navegador local a través de redireccionamientos SSH hacia `127.0.0.1:8788` y `127.0.0.1:8789`.

## Stack Recomendado

Para la mayoría de los operadores hoy en día:

- ejecutar en un VPS cerca de los endpoints del proveedor y los RPCs que realmente utilices
- Ubicación VPS UE: Frankfurt o Ámsterdam
- Ubicación VPS EE. UU.: Área de Nueva York / Newark para los endpoints predeterminados del lado este, o área de Salt Lake City para usuarios del oeste que configuren el endpoint del proveedor de Salt Lake
- Ubicación VPS Asia: Singapur o Tokio
- [Helius Developer tier](https://www.helius.dev/pricing), aproximadamente $50/mes, para la infraestructura principal
- `SOLANA_RPC_URL`: Helius Gatekeeper HTTP, `https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY`
- `SOLANA_WS_URL`: Helius standard websocket, `wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY`
- `WARM_RPC_URL`: un RPC de [Shyft](https://shyft.to/) separado para tráfico de calentamiento/caché compatible, fuera del presupuesto principal de Helius
- proveedor de ejecución: `Helius Sender` o `Hello Moon`

Por qué esta división: Helius Gatekeeper HTTP ha sido la mejor ruta HTTP de Helius en nuestras pruebas, mientras que el websocket estándar de Helius ha sido la mejor ruta de websocket de observación. Shyft es un buen RPC de calentamiento de baja prioridad porque su nivel gratuito es útil para el calentamiento, la caché y el tráfico de altura de bloque.

Hello Moon es el proveedor alternativo de baja latencia recomendado. Requiere acceso a Lunar Lander desde la [documentación de Hello Moon](https://docs.hellomoon.io/reference/lunar-lander) o el [Discord de Hello Moon](https://discord.com/invite/HelloMoon).

No consideres ningún número de latencia compartido como universal. Prueba desde el VPS y la región donde realmente operes.

### Nota sobre el VPS

[Vultr](https://www.vultr.com/?ref=9589308) es el ejemplo probado en [docs/VPS_SETUP.md](docs/VPS_SETUP.md). Es fácil de desplegar rápidamente en muchas regiones, admite pagos estándar con tarjeta/fiat así como criptomonedas, y ha sido confiable para uso a largo plazo. Cualquier otro proveedor de VPS está bien, siempre y cuando lo ubiques cerca de los endpoints del proveedor y los RPCs que planees usar.

Nota personal: He usado Vultr durante más de 5 años y no he tenido problemas con él.

## Empieza Aquí

Para la mayoría de los usuarios:

1. Lee [docs/QUICKSTART.md](docs/QUICKSTART.md) para la configuración local en Windows/Linux.
2. Si estás utilizando un servidor nuevo, utiliza [docs/VPS_SETUP.md](docs/VPS_SETUP.md) en su lugar.
3. Instala la extensión del navegador con [docs/EXTENSION.md](docs/EXTENSION.md). Para la mayoría de los usuarios, descarga el archivo `trench-tools-extension.zip` más reciente, descomprímelo y carga la carpeta descomprimida como una extensión de Chrome/Edge no empaquetada.
4. Ten a mano [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) para problemas de conexión/autenticación.

Con un VPS nuevo, el script de arranque bootstrap y un plan Helius Developer tier, la mayoría de los usuarios pueden poner en marcha el stack desde cero en unos 5-10 minutos. La configuración local es adecuada cuando estés editando, probando o aprendiendo a usar la herramienta.

Si te quedas atascado durante la configuración, utiliza un asistente de codificación con IA para guiarte en los pasos. [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI), Codex, Claude y herramientas similares son adecuados para verificar comandos de instalación, editar el `.env`, leer registros y seguir la guía del VPS.

## Modos de Runtime

El archivo `.env.example` inicial ejecuta el stack completo por defecto:

```bash
TRENCH_TOOLS_MODE=both
```

Usa `both` para la configuración normal. Inicia `execution-engine`, `launchdeck-engine` y `launchdeck-follow-daemon`.

Otros modos:

- `ee` - solo trading a través de la extensión. Inicia `execution-engine` en el puerto `8788`.
- `ld` - solo LaunchDeck. Inicia `launchdeck-engine` en el puerto `8789` y `launchdeck-follow-daemon` en el puerto `8790`.

Ejecuta desde la raíz del repositorio:

```bash
npm start
npm stop
npm restart
```

El lanzador se cierra después de que las comprobaciones de salud (health checks) pasen, y los servicios seleccionados siguen ejecutándose en segundo plano. Usa `npm stop` para detenerlos.

Para anular el modo en una ejecución única:

```bash
# Windows
.\trench-tools-start.ps1 --mode both

# Linux
./trench-tools-start.sh --mode both
```

## Sitios Compatibles

La lista de sitios de la extensión cambia rápidamente. Soporte actual implementado:

- Live: `axiom.trade` / `backup.axiom.trade`
- Live: `j7tracker.io`
- Live: `x.com`
- Planeado: Terminal (anteriormente Padre), GMGN, Telegram web, Discord web y más terminales

Consulta [docs/EXTENSION.md](docs/EXTENSION.md) para ver los pasos de instalación actuales, los interruptores de sitios y las interfaces específicas de cada plataforma.

## Cobertura de Rutas Actual

El motor de ejecución verifica las rutas desde el estado on-chain antes de operar. La cobertura nativa actual incluye la curva de enlace de Pump, Pump AMM, launchpad LetsBonk y rutas de Bonk post-migración compatibles, pools de SOL de Raydium LaunchLab, entradas de pool de Raydium AMM v4 y CPMM WSOL, Meteora DBC, Meteora DAMM v2, y una pequeña lista blanca de rutas estables confiables.

El soporte para Raydium AMM v4 y CPMM no significa un descubrimiento genérico del mejor pool. Los pools independientes de Raydium deben enviarse como cuentas de pool verificadas y pasar las comprobaciones de propietario/diseño/mint.

El soporte de pools/pares no es intencionadamente lo mismo que "cualquier cosa que un sitio web etiquete como un par". Consulta [docs/SUPPORTED_POOLS.md](docs/SUPPORTED_POOLS.md) antes de asumir que una ruta es ejecutable.

## Tarifa de Soporte Voluntaria

Trench Tools tiene un ajuste de tarifa voluntaria en las rutas de trade compatibles. El valor es un porcentaje, por lo que `0.1` significa `0.1%`. Por ejemplo, `100 SOL` en volumen sumarían `0.1 SOL` en tarifas de soporte al `0.1%`. La tarifa ayuda a mantener el desarrollo y mantenimiento continuo.

El archivo `.env.example` inicial utiliza el valor predeterminado:

```bash
TRENCH_TOOL_FEE=0.1
```

Para desactivarlo:

```bash
TRENCH_TOOL_FEE=0
```

Para aumentarlo al `0.2%`:

```bash
TRENCH_TOOL_FEE=0.2
```

Reinicia el runtime después de cambiar el `.env`. El ajuste solo se aplica a las rutas de trade compatibles que incluyan la ruta de tarifa de Trench Tools.

## Verificación Rápida

Después de la configuración:

- `execution-engine` es accesible en `http://127.0.0.1:8788`
- `launchdeck-engine` es accesible en `http://127.0.0.1:8789` cuando se ejecuta `both` o `ld`
- `launchdeck-follow-daemon` se está ejecutando detrás de LaunchDeck cuando se ejecuta `both` o `ld`
- el archivo de token existe en `.local/trench-tools/default-engine-token.txt`
- Opciones de la Extensión -> Configuración Global muestra el estado de conexión al host esperado
- Axiom, J7Tracker o X muestran las interfaces habilitadas de Trench Tools
- el popup de la barra de herramientas muestra el preset, la billetera/grupo y los controles de compra rápida esperados

Si el runtime está en un VPS y tu navegador está en tu propia computadora, agrega ambos redireccionamientos a tu configuración de SSH para que [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI)/SSH los abra automáticamente:

```sshconfig
Host Trenchtools-vps
  HostName YOUR_SERVER_IP
  User root
  LocalForward 8788 127.0.0.1:8788
  LocalForward 8789 127.0.0.1:8789
  ExitOnForwardFailure yes
  ServerAliveInterval 30
```

Alternativa manual:

```bash
ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

Usa una cantidad pequeña de prueba primero. Comienza con los proveedores recomendados: `Helius Sender` o `Hello Moon`.

## Mapa de Documentación

Empieza aquí:

- [docs/QUICKSTART.md](docs/QUICKSTART.md) - configuración local en Windows/Linux, primera ejecución y primera conexión de la extensión
- [docs/VPS_SETUP.md](docs/VPS_SETUP.md) - configuración de VPS nuevo, script de bootstrap, servicio systemd, túneles SSH
- [docs/EXTENSION.md](docs/EXTENSION.md) - instalación en modo desarrollador de Chrome/Edge, emparejamiento de host, token de autenticación, presets, sitios, actualizaciones
- [docs/CONFIG.md](docs/CONFIG.md) - stack recomendado, valores predeterminados del runtime, guía de Helius, regiones, comportamiento de calentamiento
- [docs/ENV_REFERENCE.md](docs/ENV_REFERENCE.md) - cada variable de `.env.example` y `.env.advanced`

Ejecución y arquitectura:

- [docs/PROVIDERS.md](docs/PROVIDERS.md) - notas sobre Helius Sender, Hello Moon y proveedores diferidos
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - motor de ejecución, extensión, LaunchDeck, flujo de autenticación, estado local
- [docs/SUPPORTED_POOLS.md](docs/SUPPORTED_POOLS.md) - pools compatibles del motor de ejecución, rutas y advertencias sobre pares
- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) - problemas de inicio, autenticación de la extensión, VPS, RPC y proveedores

LaunchDeck:

- [docs/launchdeck/USAGE.md](docs/launchdeck/USAGE.md) - flujo de trabajo del operador de LaunchDeck
- [docs/launchdeck/LAUNCHPADS.md](docs/launchdeck/LAUNCHPADS.md) - matriz de soporte de Pump, Bonk, Bagsapp
- [docs/launchdeck/STRATEGIES.md](docs/launchdeck/STRATEGIES.md) - compras de dev, snipes, ventas de dev, ventas de seguimiento (follow sells)
- [docs/launchdeck/METADATA_AND_VANITY.md](docs/launchdeck/METADATA_AND_VANITY.md) - cargas de metadatos/IPFS, Pinata y colas de mint vanity
- [docs/launchdeck/FOLLOW_DAEMON.md](docs/launchdeck/FOLLOW_DAEMON.md) - propiedad del observador, disparadores y temporización de seguimiento
- [docs/launchdeck/REPORTING.md](docs/launchdeck/REPORTING.md) - reportes, historial y estado local

Referencia interna/contribuyentes:

- [docs/internal/EXECUTION_DOS_AND_DONTS.md](docs/internal/EXECUTION_DOS_AND_DONTS.md)
- [docs/internal/ROUTE_SOURCE_POLICY.md](docs/internal/ROUTE_SOURCE_POLICY.md)

## Seguridad

Mantén el runtime privado por defecto:

- no compartas el archivo `.env`
- no pegues claves privadas reales, claves API, JWT o tokens de autenticación en issues, capturas de pantalla, Discord o mensajes de soporte
- no expongas puertos locales raw a la internet pública
- usa el patrón de túnel SSH para VPS en [docs/VPS_SETUP.md](docs/VPS_SETUP.md)
- usa HTTPS y concesiones de permisos de host del navegador si diriges intencionadamente la extensión a hosts que no sean loopback

Lee [SECURITY.md](SECURITY.md) antes de ejecutar esto con billeteras reales.

## Licencia

El software principal de Trench.Tools, incluyendo el motor de ejecución, la extensión del navegador, LaunchDeck y los crates de runtime compartidos, tiene licencia exclusiva bajo la Licencia Pública General Affero de GNU v3.0 (AGPLv3). Consulta [LICENSE](LICENSE) para el texto completo de la licencia y [NOTICE](NOTICE) para los avisos del proyecto.

Eres libre de usar, estudiar, modificar, auto-alojar y redistribuir el software bajo los términos de la AGPLv3. Si modificas el software y lo pones a disposición de otros, ya sea a través de un servicio alojado o un producto accesible por red, debes poner el código fuente correspondiente a disposición bajo la misma licencia.

## Branding

El nombre, logo, dominio, identidad visual y branding relacionado de Trench.Tools no están licenciados bajo la AGPLv3. No puedes utilizar la marca Trench.Tools para presentar un fork, versión modificada, servicio comercial, servicio alojado, paquete de extensión o producto no relacionado como oficial, respaldado, patrocinado o afiliado a Trench.Tools sin permiso por escrito.

Consulta [TRADEMARK.md](TRADEMARK.md) para las directrices de marca registrada y branding.
