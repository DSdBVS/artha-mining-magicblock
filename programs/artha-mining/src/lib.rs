// programs/artha-mining/src/lib.rs
//
// ARTHA MINING — контракт под MagicBlock Real-Time Hackathon
// Idea 2: Mining game (Ore-style competitive mining loop, real time)
//
// Хуки MagicBlock перенесены из реального рабочего примера:
// magicblock-engine-examples/rewards-delegated-vrf (файлы delegate_reward_list.rs
// и lib.rs, изучены построчно 03.08.2026).
//
// ВАЖНОЕ УТОЧНЕНИЕ по сравнению с первой версией: VRF в MagicBlock — АСИНХРОННЫЙ.
// Нельзя одним вызовом передать randomness_result — сначала request (запрос
// случайности у oracle), потом отдельным callback-вызовом consume (получение
// готового результата). Поэтому mine_tick разделён на request_mine_tick +
// consume_mine_tick.
//
// Остаётся один настоящий TODO: реальный CPI mint_to/transfer BOBBY/RABBIT
// токенов в claim_rewards (сейчас только эмитит событие ClaimEvent).
//
// Сюжет: игрок выбирает фракцию (Bobby или Black Rabbit), майнит "камни"
// тиками внутри Ephemeral Rollup. Каждый тик — шанс на редкую находку (VRF).

use anchor_lang::prelude::*;
use ephemeral_rollups_sdk::anchor::{commit, delegate, ephemeral};
use ephemeral_rollups_sdk::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_rollups_sdk::cpi::DelegateConfig;
use ephemeral_vrf_sdk::anchor::vrf;
use ephemeral_vrf_sdk::instructions::{create_request_randomness_ix, RequestRandomnessParams};
use ephemeral_vrf_sdk::types::SerializableAccountMeta;

declare_id!("3w3xpXVkB6L5w1rSfNawsVT771t5oPc4XJcXZATUzrkP");

// Реальные mint-адреса уже существующих токенов (Solana Devnet)
pub mod bobby_mint {
    use anchor_lang::prelude::*;
    declare_id!("LxUpczgFu1jE5QmRcRhjYgW3fP5MV3nGm1woJQsFR5a");
}
pub mod rabbit_mint {
    use anchor_lang::prelude::*;
    declare_id!("2mAjpRkrthCAtA2VjhBiWL9pem4QmbzBTgTCmHn6Rsij");
}
// Оба токена: decimals = 6

#[ephemeral]
#[program]
pub mod artha_mining {
    use super::*;

    /// Создаёт майнинг-аккаунт игрока и закрепляет фракцию.
    pub fn initialize_miner(ctx: Context<InitializeMiner>, faction: Faction) -> Result<()> {
        let miner = &mut ctx.accounts.miner;
        miner.owner = ctx.accounts.player.key();
        miner.faction = faction;
        miner.total_ore = 0;
        miner.rare_finds = 0;
        miner.last_mine_ts = Clock::get()?.unix_timestamp;
        miner.bump = ctx.bumps.miner;
        Ok(())
    }

    /// Делегирует miner-аккаунт в Ephemeral Rollup — с этого момента
    /// mine_tick/request_mine_tick выполняются быстро и без газа внутри ER.
    pub fn delegate_miner(ctx: Context<DelegateMiner>) -> Result<()> {
        ctx.accounts.delegate_miner(
            &ctx.accounts.player,
            &[b"miner", ctx.accounts.player.key().as_ref()],
            DelegateConfig {
                validator: ctx.remaining_accounts.first().map(|acc| acc.key()),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// Шаг 1 из 2: запрос случайности у VRF-oracle для одного тика майнинга.
    /// Выполняется внутри ER. Результат придёт позже отдельным callback-ом
    /// в consume_mine_tick — это НЕ синхронный вызов.
    pub fn request_mine_tick(ctx: Context<RequestMineTick>, client_seed: u8) -> Result<()> {
        msg!("Requesting randomness for mine tick...");

        // DelegationRecord layout: [8 discriminator][32 authority (validator)][...]
        // Читаем validator напрямую из сырых байт, не импортируя структуру.
        let delegation_record_data = ctx.accounts.delegation_record_miner.try_borrow_data()?;
        require!(
            delegation_record_data.len() >= 40,
            MiningError::InvalidDelegationRecord
        );
        let validator = Pubkey::try_from(&delegation_record_data[8..40])
            .map_err(|_| error!(MiningError::InvalidDelegationRecord))?;
        drop(delegation_record_data);

        // Seeds: ["magic-fee-vault", validator] под delegation program
        let (magic_fee_vault, _) = Pubkey::find_program_address(
            &[b"magic-fee-vault", validator.as_ref()],
            &ephemeral_rollups_sdk::id(),
        );

        let ix = create_request_randomness_ix(RequestRandomnessParams {
            payer: ctx.accounts.player.key(),
            oracle_queue: ctx.accounts.oracle_queue.key(),
            callback_program_id: ID,
            callback_discriminator: crate::instruction::ConsumeMineTick::DISCRIMINATOR.to_vec(),
            caller_seed: [client_seed; 32],
            accounts_metas: Some(vec![
                SerializableAccountMeta {
                    pubkey: ctx.accounts.miner.key(),
                    is_signer: false,
                    is_writable: true,
                },
                SerializableAccountMeta {
                    pubkey: magic_fee_vault,
                    is_signer: false,
                    is_writable: true,
                },
                SerializableAccountMeta {
                    pubkey: MAGIC_PROGRAM_ID,
                    is_signer: false,
                    is_writable: false,
                },
                SerializableAccountMeta {
                    pubkey: MAGIC_CONTEXT_ID,
                    is_signer: false,
                    is_writable: true,
                },
            ]),
            ..Default::default()
        });
        ctx.accounts
            .invoke_signed_vrf(&ctx.accounts.player.to_account_info(), &ix)?;
        msg!(
            "VRF randomness request triggered for mine_tick, player: {:?}",
            ctx.accounts.player.key()
        );
        Ok(())
    }

    /// Шаг 2 из 2: callback от VRF-oracle с готовой случайностью.
    /// Здесь и происходит собственно начисление руды + проверка на редкую находку.
    pub fn consume_mine_tick(ctx: Context<ConsumeMineTick>, randomness: [u8; 32]) -> Result<()> {
        let miner = &mut ctx.accounts.miner;
        let now = Clock::get()?.unix_timestamp;

        const BASE_YIELD: u64 = 5;
        const STRIKE_CHANCE_BPS: u64 = 1000; // 10%
        const STRIKE_MULTIPLIER: u64 = 10;

        // Берём первые 8 байт случайности как u64
        let roll_source = u64::from_le_bytes(randomness[0..8].try_into().unwrap());
        let roll = roll_source % 10_000;
        let is_strike = roll < STRIKE_CHANCE_BPS;

        let yield_amount = if is_strike {
            miner.rare_finds = miner.rare_finds.saturating_add(1);
            BASE_YIELD.saturating_mul(STRIKE_MULTIPLIER)
        } else {
            BASE_YIELD
        };

        miner.total_ore = miner.total_ore.saturating_add(yield_amount);
        miner.last_mine_ts = now;

        emit!(MineTickEvent {
            player: miner.owner,
            faction: miner.faction,
            yield_amount,
            is_strike,
            total_ore: miner.total_ore,
        });

        Ok(())
    }

    /// Снимает делегирование, фиксирует финальное состояние обратно в Solana.
    /// TODO: точный вызов undelegate пока не найден — нужен файл
    /// instructions/undelegate_reward_list.rs из их примера, чтобы увидеть,
    /// как именно вызывается undelegate (не через сгенерированный ctx-метод,
    /// как с delegate — тут, похоже, что-то другое).
    pub fn undelegate_miner(_ctx: Context<UndelegateMiner>) -> Result<()> {
        // TODO: см. instructions/undelegate_reward_list.rs
        Ok(())
    }

    /// Клейм наград на базовом слое Solana. Начисляет total_ore игроку
    /// в токене его фракции (BOBBY или RABBIT). reward_mint в контексте
    /// ДОЛЖЕН совпадать с miner.faction.mint() (constraint ниже) —
    /// перепутать фракцию/токен нельзя.
    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        let miner = &mut ctx.accounts.miner;
        require!(miner.total_ore > 0, MiningError::NothingToClaim);

        let claimed = miner.total_ore;

        // TODO: реальный CPI mint_to/transfer из BOBBY/RABBIT mint-а
        // на token account игрока (anchor_spl::token::{mint_to, MintTo}).
        // Оба токена: decimals=6, mint authority = artha-devnet.json.

        emit!(ClaimEvent {
            player: miner.owner,
            faction: miner.faction,
            claimed_ore: claimed,
        });

        miner.total_ore = 0;
        Ok(())
    }
}

#[account]
pub struct MinerAccount {
    pub owner: Pubkey,
    pub faction: Faction,
    pub total_ore: u64,
    pub rare_finds: u32,
    pub last_mine_ts: i64,
    pub bump: u8,
}

impl MinerAccount {
    pub const SIZE: usize = 8 + 32 + 1 + 8 + 4 + 8 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum Faction {
    Bobby,
    BlackRabbit,
}

impl Faction {
    /// Возвращает mint-адрес токена, соответствующего фракции.
    pub fn mint(&self) -> Pubkey {
        match self {
            Faction::Bobby => bobby_mint::ID,
            Faction::BlackRabbit => rabbit_mint::ID,
        }
    }
}

#[derive(Accounts)]
pub struct InitializeMiner<'info> {
    #[account(mut)]
    pub player: Signer<'info>,

    #[account(
        init,
        payer = player,
        space = MinerAccount::SIZE,
        seeds = [b"miner", player.key().as_ref()],
        bump
    )]
    pub miner: Account<'info, MinerAccount>,

    pub system_program: Program<'info, System>,
}

/// Аккаунт под делегирование помечен `del` — после делегирования владелец
/// PDA меняется на Delegation Program, поэтому Account<T> с проверкой
/// владельца сломался бы; используем UncheckedAccount (как в их примере).
#[delegate]
#[derive(Accounts)]
pub struct DelegateMiner<'info> {
    #[account(mut)]
    pub player: Signer<'info>,
    /// CHECK: делегируемый PDA майнера
    #[account(mut, del, seeds = [b"miner", player.key().as_ref()], bump)]
    pub miner: UncheckedAccount<'info>,
}

#[vrf]
#[derive(Accounts)]
pub struct RequestMineTick<'info> {
    #[account(mut)]
    pub player: Signer<'info>,
    #[account(mut, seeds = [b"miner", player.key().as_ref()], bump = miner.bump)]
    pub miner: Account<'info, MinerAccount>,
    /// CHECK: адрес проверяется constraint-ом против известной VRF-очереди
    #[account(mut, address = ephemeral_vrf_sdk::consts::DEFAULT_EPHEMERAL_QUEUE)]
    pub oracle_queue: UncheckedAccount<'info>,
    /// CHECK: delegation record для miner — нужен для вычисления magic_fee_vault
    #[account(address = ephemeral_rollups_sdk::pda::delegation_record_pda_from_delegated_account(&miner.key()))]
    pub delegation_record_miner: UncheckedAccount<'info>,
}

#[commit]
#[derive(Accounts)]
pub struct ConsumeMineTick<'info> {
    #[account(address = ephemeral_vrf_sdk::consts::VRF_PROGRAM_IDENTITY)]
    pub vrf_program_identity: Signer<'info>,
    #[account(mut, seeds = [b"miner", miner.owner.as_ref()], bump = miner.bump)]
    pub miner: Account<'info, MinerAccount>,
    /// CHECK: Magic fee vault, производный от validator в delegation record
    #[account(mut)]
    pub magic_fee_vault: UncheckedAccount<'info>,
}

#[commit]
#[derive(Accounts)]
pub struct UndelegateMiner<'info> {
    #[account(mut)]
    pub player: Signer<'info>,
    #[account(mut, seeds = [b"miner", player.key().as_ref()], bump = miner.bump)]
    pub miner: Account<'info, MinerAccount>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub player: Signer<'info>,
    #[account(mut, seeds = [b"miner", player.key().as_ref()], bump = miner.bump)]
    pub miner: Account<'info, MinerAccount>,

    /// Mint токена, соответствующего фракции игрока.
    /// constraint не даст передать BOBBY-mint для игрока Black Rabbit и наоборот.
    #[account(constraint = reward_mint.key() == miner.faction.mint() @ MiningError::WrongMintForFaction)]
    pub reward_mint: Account<'info, anchor_spl::token::Mint>,

    // TODO: добавить player_token_account (ATA) + token_program,
    // когда будет реальный CPI mint_to/transfer.
}

#[event]
pub struct MineTickEvent {
    pub player: Pubkey,
    pub faction: Faction,
    pub yield_amount: u64,
    pub is_strike: bool,
    pub total_ore: u64,
}

#[event]
pub struct ClaimEvent {
    pub player: Pubkey,
    pub faction: Faction,
    pub claimed_ore: u64,
}

#[error_code]
pub enum MiningError {
    #[msg("Mining tick too soon, wait before mining again")]
    TooSoon,
    #[msg("Nothing to claim")]
    NothingToClaim,
    #[msg("Reward mint does not match player's faction (BOBBY vs BLACK RABBIT)")]
    WrongMintForFaction,
    #[msg("Invalid delegation record for miner account")]
    InvalidDelegationRecord,
}
