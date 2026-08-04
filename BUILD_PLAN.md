# ARTHA Mining — план сборки под MagicBlock Real-Time Hackathon

## Уже существующие токены (подключены в контракте)

| | BOBBY | BLACK RABBIT |
|---|---|---|
| Program ID | `Bc8L4WEfz4paixRSuu39jFwkyqt8yF2TsQJ7A5gdxhZb` | `6fWHvoB3fN8Pf2eLo4YsVALJK3fvWvRWqJWHAvfY4Gzj` |
| Mint | `LxUpczgFu1jE5QmRcRhjYgW3fP5MV3nGm1woJQsFR5a` | `2mAjpRkrthCAtA2VjhBiWL9pem4QmbzBTgTCmHn6Rsij` |
| Decimals | 6 | 6 |
| Cluster | Devnet | Devnet |
| Mint authority | `artha-devnet.json` | `artha-devnet.json` |

## Что уже готово (от Лолиты)
- `programs/artha-mining/src/lib.rs` — игровая логика:
  - `initialize_miner(faction)` — создание майнера, выбор фракции Bobby/Black Rabbit
  - `mine_tick(randomness_result)` — один тик добычи, 10% шанс на редкую находку (x10 множитель)
  - `claim_rewards()` — клейм намайненного, **привязан к реальным mint-адресам** BOBBY/RABBIT выше, с on-chain constraint (нельзя перепутать фракцию с токеном)
  - `delegate_miner()` / `undelegate_miner()` — ЗАГЛУШКИ, нужно заполнить реальными вызовами MagicBlock

## Что нужно сделать на Mac (по порядку)

### 1. Клонировать официальные примеры MagicBlock
```
cd ~/Desktop
git clone https://github.com/magicblock-labs/starter-kits.git
git clone https://github.com/magicblock-labs/magicblock-engine-examples.git
```

### 2. Поставить AI Dev Skill для Claude Code (если работаешь через Claude Code локально)
```
npx add-skill https://github.com/magicblock-labs/magicblock-dev-skill
```
Это даст агенту актуальные паттерны delegate/commit/undelegate/VRF — не придётся гадать API.

### 3. Изучить два примера:
- `magicblock-engine-examples/anchor-counter` — САМЫЙ ПРОСТОЙ пример delegate/undelegate. Смотрим, как именно оформлены:
  - `#[delegate]` макрос на аккаунте
  - CPI на Delegation Program (`DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh`)
  - инструкция `undelegate`
- `starter-kits/vrf-demo` — пример "rolling for character stats". Смотрим:
  - как запрашивается randomness (request)
  - как приходит callback с результатом
  - это и есть паттерн для нашего `mine_tick(randomness_result)`

### 4. Перенести хуки в artha-mining/src/lib.rs
Заменить TODO-блоки:
- `DelegateMiner` accounts + `delegate_miner()` тело — скопировать паттерн из anchor-counter
- `MineTick` accounts + вызов VRF — скопировать паттерн из vrf-demo, подставить наш `mine_tick` вместо их "roll for stats"
- `UndelegateMiner` — аналогично anchor-counter

### 5. Собрать и задеплоить (как обычно)
```
cd artha-mining
anchor build
anchor deploy --provider.cluster devnet
```

### 6. Подключить к ER-эндпоинту при тестировании
```javascript
const providerEphemeralRollup = new anchor.AnchorProvider(
  new anchor.web3.Connection(
    "https://devnet.magicblock.app/",
    { wsEndpoint: "wss://devnet.magicblock.app/" }
  ),
  anchor.Wallet.local()
);
```

## Дальше (после рабочего MVP)
- Простой UI на базе уже готового ARTHA Wallet — экран выбора фракции + кнопка "Mine"
- Демо-видео для сабмишна (30-60 сек: выбор фракции → несколько тиков → редкая находка → claim)
- Текст сабмишна на realtime.magicblock.app

## Дедлайн
Сабмишн до воскресенья, 9 августа 2026.
