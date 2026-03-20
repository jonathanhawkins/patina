# Oracle Parity Report — Godot 4.6.1 vs Patina

Generated: 2026-03-20

## `minimal.tscn` — 1/1 (100.0%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/Root` | `_class` | `"Node"` |

*No mismatches — perfect parity.*

---

## `hierarchy.tscn` — 3/3 (100.0%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/Root` | `_class` | `"Node"` |
| `/root/Root/Player` | `_class` | `"Node2D"` |
| `/root/Root/Player/Sprite` | `_class` | `"Sprite2D"` |

*No mismatches — perfect parity.*

---

## `with_properties.tscn` — 5/5 (100.0%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/Root` | `_class` | `"Node"` |
| `/root/Root/Player` | `_class` | `"Node2D"` |
| `/root/Root/Player` | `position` | `{"type":"Vector2","value":[100.0,200.0]}` |
| `/root/Root/Background` | `_class` | `"Node2D"` |
| `/root/Root/Background` | `modulate` | `{"type":"Color","value":[0.200000002980232,0.400000005960…` |

*No mismatches — perfect parity.*

---

## `space_shooter.tscn` — 8/13 (61.5%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/SpaceShooter` | `_class` | `"Node2D"` |
| `/root/SpaceShooter/Background` | `_class` | `"Node2D"` |
| `/root/SpaceShooter/Player` | `_class` | `"Node2D"` |
| `/root/SpaceShooter/Player` | `position` | `{"type":"Vector2","value":[320.0,400.0]}` |
| `/root/SpaceShooter/EnemySpawner` | `_class` | `"Node2D"` |
| `/root/SpaceShooter/EnemySpawner` | `position` | `{"type":"Vector2","value":[320.0,0.0]}` |
| `/root/SpaceShooter/ScoreLabel` | `_class` | `"Node2D"` |
| `/root/SpaceShooter/ScoreLabel` | `position` | `{"type":"Vector2","value":[10.0,10.0]}` |

### Mismatched properties

| Node | Property | Godot | Patina |
|------|----------|-------|--------|
| `/root/SpaceShooter/Player` | `can_shoot` | `{"type":"bool","value":true}` | `(missing)` |
| `/root/SpaceShooter/Player` | `shoot_cooldown` | `{"type":"float","value":0.0}` | `(missing)` |
| `/root/SpaceShooter/Player` | `speed` | `{"type":"float","value":200.0}` | `(missing)` |
| `/root/SpaceShooter/EnemySpawner` | `spawn_interval` | `{"type":"float","value":2.0}` | `(missing)` |
| `/root/SpaceShooter/EnemySpawner` | `spawn_timer` | `{"type":"float","value":0.0}` | `(missing)` |

---

## `platformer.tscn` — 12/12 (100.0%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/World` | `_class` | `"Node"` |
| `/root/World/Player` | `_class` | `"Node2D"` |
| `/root/World/Player` | `position` | `{"type":"Vector2","value":[100.0,300.0]}` |
| `/root/World/Platform1` | `_class` | `"Node2D"` |
| `/root/World/Platform1` | `position` | `{"type":"Vector2","value":[0.0,500.0]}` |
| `/root/World/Platform2` | `_class` | `"Node2D"` |
| `/root/World/Platform2` | `position` | `{"type":"Vector2","value":[300.0,400.0]}` |
| `/root/World/Platform3` | `_class` | `"Node2D"` |
| `/root/World/Platform3` | `position` | `{"type":"Vector2","value":[600.0,350.0]}` |
| `/root/World/Camera` | `_class` | `"Camera2D"` |
| `/root/World/Collectible` | `_class` | `"Node2D"` |
| `/root/World/Collectible` | `position` | `{"type":"Vector2","value":[450.0,250.0]}` |

*No mismatches — perfect parity.*

---

## `physics_playground.tscn` — 12/12 (100.0%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/World` | `_class` | `"Node"` |
| `/root/World/Ball` | `_class` | `"RigidBody2D"` |
| `/root/World/Ball` | `position` | `{"type":"Vector2","value":[400.0,100.0]}` |
| `/root/World/Ball/CollisionShape` | `_class` | `"CollisionShape2D"` |
| `/root/World/Wall` | `_class` | `"StaticBody2D"` |
| `/root/World/Wall` | `collision_mask` | `{"type":"int","value":0}` |
| `/root/World/Wall` | `position` | `{"type":"Vector2","value":[800.0,300.0]}` |
| `/root/World/Wall/CollisionShape` | `_class` | `"CollisionShape2D"` |
| `/root/World/Floor` | `_class` | `"StaticBody2D"` |
| `/root/World/Floor` | `collision_mask` | `{"type":"int","value":0}` |
| `/root/World/Floor` | `position` | `{"type":"Vector2","value":[400.0,600.0]}` |
| `/root/World/Floor/CollisionShape` | `_class` | `"CollisionShape2D"` |

*No mismatches — perfect parity.*

---

## `signals_complex.tscn` — 9/9 (100.0%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/Root` | `_class` | `"Node"` |
| `/root/Root/Player` | `_class` | `"Node2D"` |
| `/root/Root/Player` | `position` | `{"type":"Vector2","value":[200.0,300.0]}` |
| `/root/Root/Player/TriggerZone` | `_class` | `"Node2D"` |
| `/root/Root/Enemy` | `_class` | `"Node2D"` |
| `/root/Root/Enemy` | `position` | `{"type":"Vector2","value":[600.0,300.0]}` |
| `/root/Root/HUD` | `_class` | `"Node"` |
| `/root/Root/ItemDrop` | `_class` | `"Node2D"` |
| `/root/Root/ItemDrop` | `position` | `{"type":"Vector2","value":[400.0,200.0]}` |

*No mismatches — perfect parity.*

---

## `test_scripts.tscn` — 4/11 (36.4%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/TestScene` | `_class` | `"Node2D"` |
| `/root/TestScene/Mover` | `_class` | `"Node2D"` |
| `/root/TestScene/VarTest` | `_class` | `"Node2D"` |
| `/root/TestScene/VarTest` | `position` | `{"type":"Vector2","value":[300.0,200.0]}` |

### Mismatched properties

| Node | Property | Godot | Patina |
|------|----------|-------|--------|
| `/root/TestScene/Mover` | `direction` | `{"type":"float","value":1.0}` | `(missing)` |
| `/root/TestScene/Mover` | `position` | `{"type":"Vector2","value":[100.0,200.0]}` | `{"type":"Vector2","value":[100.833335…` |
| `/root/TestScene/Mover` | `speed` | `{"type":"float","value":50.0}` | `(missing)` |
| `/root/TestScene/VarTest` | `health` | `{"type":"int","value":100}` | `(missing)` |
| `/root/TestScene/VarTest` | `is_alive` | `{"type":"bool","value":true}` | `(missing)` |
| `/root/TestScene/VarTest` | `name_str` | `{"type":"String","value":"Player"}` | `(missing)` |
| `/root/TestScene/VarTest` | `velocity` | `{"type":"Vector2","value":[0.0,0.0]}` | `(missing)` |

---

## `ui_menu.tscn` — 5/5 (100.0%)

### Matching properties

| Node | Property | Value |
|------|----------|-------|
| `/root/MenuRoot` | `_class` | `"Node"` |
| `/root/MenuRoot/Title` | `_class` | `"Node"` |
| `/root/MenuRoot/PlayButton` | `_class` | `"Node"` |
| `/root/MenuRoot/SettingsButton` | `_class` | `"Node"` |
| `/root/MenuRoot/QuitButton` | `_class` | `"Node"` |

*No mismatches — perfect parity.*

---

## Summary

**Overall**: 59/71 (83.1%)
