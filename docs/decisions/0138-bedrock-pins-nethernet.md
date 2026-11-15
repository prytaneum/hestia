# Bedrock pins NetherNet, and status comes from the console

*Applies to: [Servers & instances](../architecture/entries.md), [Minecraft providers](../architecture/minecraft.md)*

BDS 1.26 ships `transport=nethernet`. Setting `transport=raknet` is accepted and
then refused at startup — the server logs "NetherNet is the only supported
transport type" and players cannot connect. An early cut followed the setting,
mapped the ports per transport and answered status with a RakNet ping; against a
real 1.26 server the ping got no answer and an unmanaged
`server-udp-ports=19132` collided between servers.

**`transport` is a managed key, written as `nethernet` before every start.**
`config set transport` is refused. The RakNet ping and the RakNet port mapping
were removed rather than kept as a dormant path.

**Ports follow from it.** NetherNet listens on TCP `server-port` (dual-stack) and
takes its client UDP port from `server-udp-ports`. The record keeps two ports:
the game port is claimed at create and probed for both TCP and UDP, and a second
UDP port is allocated from 19133 at start and written to `server-udp-ports` and
`server-portv6` together. `ServerInfo.ports` is a list of `ServerPort { port,
protocol }`, game port first, so a front-end says what to forward and over which
protocol.

**Status reads the console.** BDS answers no ping: online and max players come
from the console's `list`, the listed name from `server-name` and the version
from the record.

**Rejected.** Following the `transport` setting, which advertises a mode the
server will not serve. Leaving the key unmanaged. Keeping the RakNet ping for a
future flip of the default.
