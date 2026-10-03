# PROTOCOL

> Document normatif (SPEC.md sections 124-146). Complété pour M1 — Network foundation.

## Transport

- **QUIC** via `quinn` (§124). Le serveur de jeu écoute exclusivement en loopback (`127.0.0.1`) ; il n'est jamais exposé publiquement.
- **Sérialisation** : Protocol Buffers via `prost` (schéma `proto/arpg.proto`, package `arpg.v1`).
- **Framing** : préfixe longueur 4 octets big-endian, charge prost. Taille max : 1 MiB (`MAX_FRAME_BYTES`) — aucune allocation proportionnelle à une taille client non bornée (§170).

## Versionnement

`ProtocolVersion = 1`. Le `ClientHello` porte `protocol_version` ; toute incompatibilité est refusée par `ServerHello { accepted: false, reject_reason }` (§125).

## Handshake (§125)

```text
ClientHello            -> ServerHello (accepted/reject)
JoinGameRequest        -> JoinGameAccepted | (drop si partie pleine)
[command loop]         <- CommandAck par commande
```

`JoinGameAccepted` contient : `game_id`, `player_id`, `player_slot`, `tick_rate` (25), `ruleset_hash`, `datapack_hash`, `server_tick`, `input_delay_ticks` (§127).

## États de session (§128)

```text
Connecting -> Authenticated -> Running -> DisconnectedGrace -> Closed
```

Une commande gameplay n'est acceptée que dans `Running` (`Session::is_command_accepted`). Pendant `DisconnectedGrace` : aucune nouvelle action, les actions engagées se terminent normalement (§146).

## Commandes (§129-134)

- `CommandEnvelope { sequence, client_tick, player_id, command }`. Le client n'impose jamais `execute_tick`.
- Le serveur traduit : `execute_tick = max(earliest_allowed, translated_client_tick)` avec input delay borné 1..4 ticks (§130-131).
- Admission : `Accepted` / `Deferred` / `RejectedTooOld` / `RejectedInvalid` / `Duplicate` — séquences monotones par joueur (§132-133).
- Canonisation : `execute_tick -> player_slot -> sequence` (§134). L'ordre d'arrivée réseau n'a aucune influence après admission.
- `CommandAck` accuse la dernière séquence traitée (prémices du ACK §135).

## Réplication (§137-143)

- `PlayerView` : sous-ensemble visible de l'état serveur (INV-013).
- `Snapshot { snapshot_id, tick, players[], last_processed_command_sequence, deltas[] }` (§143).
- **Revisions** (§141) : chaque vue répliquée possède une `revision` incrémentée à chaque mutation (`ReplicationTracker::touch`). Les deltas ne sont produits que pour les entités dont la revision serveur dépasse la base acquittée du client.
- **Delta** (§141) : `EntityDelta { entity_id, base_revision, new_revision, field_mask, player_view }`. Après ACK client (`ReplicationTracker::acknowledge`), plus aucun delta n'est émis sans nouvelle mutation.
- **Resync** (§142) : si `base_revision != client_revision`, le client émet `EntityResyncRequest` et le serveur répond `EntityResyncResponse` avec l'état complet et la revision serveur.

## Datagrams non fiables (§136)

Les commandes de **mouvement** peuvent transiter en datagram QUIC (perte tolérée : une intention de mouvement plus récente supplante une ancienne). Toute commande avec effet de bord (inventaire, trade, compétences) reste sur le stream fiable. Le serveur n'accepte que `Move` sur le chemin datagram.

## Réseau chaotique (§186)

Le harness (`crates/arpg-sim/tests/network_chaos.rs`) valide que pour une même séquence de commandes finalement acceptées, le hash serveur est indépendant du timing d'arrivée (latence/jitter), des duplications et des pertes-avec-retransmission (fenêtre d'envoi bloquée sur le paquet perdu, séquences monotones par joueur préservées).

## Ce qui reste hors de ce document

L'interest management (§140) et la prédiction client (§144-145) sont planifiés avec les milestones suivants (M3+).
