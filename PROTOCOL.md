# PROTOCOL.md

Document normatif (SPEC.md §200). Machine à états du protocole, messages, révisions, ACK, resync, timeouts, compatibilité. Toute divergence entre ce document et le code est un bug.

## Transport

- **QUIC** via `quinn` ; sérialisation **Protocol Buffers via prost** (§124).
- Framing : préfixe longueur 4 octets big-endian, puis payload prost (`arpg-server/src/quic.rs::encode_frame/decode_frame`). Taille max par frame : `MAX_FRAME_BYTES` = 1 MiB.
- Streams fiables (§135) : handshake, join/leave, inventaire, trade, quêtes, chat, gestion de personnages, resync.
- Datagrams (§136) : commandes de mouvement, combat fréquent, deltas de mouvement. Perte autorisée.

## Machine à états de session (§128, `arpg-server/src/session.rs`)

```
Connecting → Authenticated → Joining → Synchronizing → Running
Running → DisconnectedGrace → Closed (ou reprise vers Synchronizing)
```

Une commande gameplay n'est acceptée que dans `Running`.

## Handshake (§125)

```
ClientHello → ServerHello → Authentication → CharacterSelection
→ JoinGameRequest → JoinGameAccepted → InitialSnapshot → ClientReady → Running
```

Refus si : protocole incompatible, ruleset incompatible, datapack incompatible, personnage incompatible, partie pleine.

- **ClientHello** (§126) : `protocol_version`, `client_build`, `supported_features`. Aucune donnée gameplay autoritaire.
- **JoinGameAccepted** (§127) : `game_id`, `player_id`, `player_slot`, `tick_rate`, `ruleset_hash`, `datapack_hash`, `server_tick`, `input_delay`.

## Commandes client (§129)

```text
CommandEnvelope { sequence: u32, client_tick: Tick, player: PlayerId, command: ClientCommand }
ClientCommand = Move | UseSkill | Interact | NoOp
```

- `sequence` monotone par joueur ; 0 invalide ; duplication et séquence trop vieille rejetées (fenêtre du scheduler).
- Le scheduler traduit en temps serveur : `execute_tick = max(client_tick, current_tick + input_delay)`, `input_delay` borné `[1, 4]` (défaut 2).
- Canonicalisation (§130-134) : `execute_tick`, puis `player_slot`, puis `sequence`. Le timing d'arrivée réseau n'a aucune influence après admission.

## Révisions et deltas (§141)

Chaque vue répliquée porte une `revision`. Un delta contient : `entity_id`, `base_revision`, `new_revision`, `field_mask`, `values`.

## ACK et resync (§142-143)

- Si `base_revision != client_revision`, le client envoie `EntityResyncRequest` ; le serveur renvoie l'état complet de l'entité.
- Snapshot global (§143) : `snapshot_id`, `tick`, état autoritaire du joueur, entités/objets/items au sol visibles, vue de quête, vue de party, `last_processed_command_sequence`.
- Prediction client (§144) : mouvement local uniquement ; le combat n'est jamais confirmé avant retour serveur.
- Réconciliation (§145) : position autoritaire + retrait des commandes acquittées.

## Versions séparées (§157)

```
EngineVersion | ProtocolVersion | DataSchemaVersion | RulesSchemaVersion
| SaveSchemaVersion | ReplayVersion
```

Aucun numéro global unique. Compatibilité vérifiée champ par champ au handshake et au chargement.

## Timeouts et déconnexions

- Déconnexion → `DisconnectedGrace` : le serveur maintient la session pendant une fenêtre de grâce ; reconnexion possible via `Synchronizing` (snapshot complet).
- Après expiration de la grâce : `Closed`, snapshot du personnage persisté, transition `Left` enregistrée dans le replay.

## Messages privés au serveur (§139)

Jamais transmis avant nécessité : RNG state, futur loot, résultat de gambling, contenu de coffre fermé, scores de ciblage AI, blackboard AI, monstres hors interest set, affixes d'un item non identifié, quest internals.
