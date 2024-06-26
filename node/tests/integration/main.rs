use aze_lib::client::{
    AzeClient,
    AzeGameMethods,
    AzeAccountTemplate,
    AzeTransactionTemplate,
    SendCardTransactionData,
    PlayBetTransactionData,
    PlayRaiseTransactionData,
    PlayCallTransactionData,
    PlayFoldTransactionData,
    PlayCheckTransactionData,
};
use aze_lib::constants::{
    BUY_IN_AMOUNT,
    SMALL_BLIND_AMOUNT,
    NO_OF_PLAYERS,
    FLOP_INDEX,
    IS_FOLD_OFFSET,
    PLAYER_BET_OFFSET,
    FIRST_PLAYER_INDEX,
    LAST_PLAYER_INDEX,
    HIGHEST_BET,
    PLAYER_INITIAL_BALANCE,
    PLAYER_BALANCE_SLOT,
    CURRENT_TURN_INDEX_SLOT,
    CHECK_COUNTER_SLOT,
    RAISER_INDEX_SLOT,
    PLAYER_STATS_SLOTS,
    HIGHEST_BET_SLOT,
    CURRENT_PHASE_SLOT
};
use aze_lib::executor::execute_tx_and_sync;
use aze_lib::utils::{ get_random_coin, load_config };
use aze_lib::notes::{ consume_notes, mint_note };
use aze_lib::storage::GameStorageSlotData;
use miden_client::{
    client::{
        accounts::{ AccountTemplate, AccountStorageMode },
        transactions::transaction_request::TransactionTemplate,
        rpc::TonicRpcClient,
    },
    config::{ ClientConfig, RpcConfig },
    errors::{ ClientError, NodeRpcClientError },
    store::sqlite_store::SqliteStore,
};
use miden_crypto::hash::rpo::RpoDigest;
use miden_crypto::FieldElement;
use miden_objects::{
    Felt,
    assets::{ TokenSymbol, FungibleAsset, Asset },
    accounts::{ Account, AccountId },
    notes::NoteType,
};
use std::{ env::temp_dir, time::Duration };
// use uuid::Uuid;

fn create_test_client() -> AzeClient {
    let mut current_dir = std::env
        ::current_dir()
        .map_err(|err| err.to_string())
        .unwrap();
    current_dir.push("miden-client.toml");
    let client_config = load_config(current_dir.as_path()).unwrap();

    println!("Client Config: {:?}", client_config);

    let rpc_endpoint = client_config.rpc.endpoint.to_string();
    let store = SqliteStore::new((&client_config).into()).unwrap();
    let executor_store = SqliteStore::new((&client_config).into()).unwrap();
    let rng = get_random_coin();
    AzeClient::new(TonicRpcClient::new(&rpc_endpoint), rng, store, executor_store, true)
}

async fn wait_for_node(client: &mut AzeClient) {
    const NODE_TIME_BETWEEN_ATTEMPTS: u64 = 5;
    const NUMBER_OF_NODE_ATTEMPTS: u64 = 60;

    println!(
        "Waiting for Node to be up. Checking every {NODE_TIME_BETWEEN_ATTEMPTS}s for {NUMBER_OF_NODE_ATTEMPTS} tries..."
    );

    for _try_number in 0..NUMBER_OF_NODE_ATTEMPTS {
        match client.sync_state().await {
            Err(ClientError::NodeRpcClientError(NodeRpcClientError::ConnectionError(_))) => {
                std::thread::sleep(Duration::from_secs(NODE_TIME_BETWEEN_ATTEMPTS));
            }
            Err(other_error) => {
                panic!("Unexpected error: {other_error}");
            }
            _ => {
                return;
            }
        }
    }

    panic!("Unable to connect to node");
}

fn setup_accounts(
    client: &mut AzeClient
) -> (Account, AccountId, AccountId, GameStorageSlotData) {
    let slot_data = GameStorageSlotData::new(
        SMALL_BLIND_AMOUNT,
        BUY_IN_AMOUNT as u8,
        NO_OF_PLAYERS,
        FIRST_PLAYER_INDEX,
        HIGHEST_BET,
        PLAYER_INITIAL_BALANCE
    );

    let (game_account, _) = client
        .new_game_account(
            AzeAccountTemplate::GameAccount {
                mutable_code: false,
                storage_mode: AccountStorageMode::Local,
            },
            Some(slot_data.clone())
        )
        .unwrap();

    let (player_account, _) = client
        .new_game_account(
            AzeAccountTemplate::PlayerAccount {
                mutable_code: false,
                storage_mode: AccountStorageMode::Local,
            },
            None
        )
        .unwrap();

    let (faucet_account, _) = client
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("MATIC").unwrap(),
            decimals: 8,
            max_supply: 1_000_000_000,
            storage_mode: AccountStorageMode::Local,
        })
        .unwrap();

    return (game_account, player_account.id(), faucet_account.id(), slot_data);
}

#[tokio::test]
async fn test_create_aze_game_account() {
    let mut client = create_test_client();

    // TODO: somehow manage the game seed as well
    let (game_account, _, _, _) = setup_accounts(&mut client);
    let game_account_storage = game_account.storage();

    let mut slot_index = 1;

    // check are the cards has been correctly placed
    for card_suit in 1..5 {
        for card_number in 1..14 {
            let slot_item = RpoDigest::new([
                Felt::from(card_suit as u8),
                Felt::from(card_number as u8),
                Felt::ZERO, // denotes is encrypted
                Felt::ZERO,
            ]);

            assert_eq!(game_account_storage.get_item(slot_index), slot_item);

            slot_index = slot_index + 1;
        }
    }

    // checking next turn
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(FLOP_INDEX), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;

    // checking the small blind amount
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(SMALL_BLIND_AMOUNT), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;

    // checking the big blind amount
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(SMALL_BLIND_AMOUNT * 2), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;

    // checking the buy in amount
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(BUY_IN_AMOUNT as u8), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;
    // checking no of player slot
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(NO_OF_PLAYERS), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;
    // checking flop index slot
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );
}

#[tokio::test]
async fn test_cards_distribution() {
    let mut client: AzeClient = create_test_client();

    let (game_account, player1_account_id, faucet_account_id, _) = setup_accounts(&mut client);

    let game_account_id = game_account.id();
    let game_account_storage = game_account.storage();

    let (player2_account, _) = client
        .new_game_account(
            AzeAccountTemplate::PlayerAccount {
                mutable_code: false,
                storage_mode: AccountStorageMode::Local,
            },
            None
        )
        .unwrap();

    fund_account(&mut client, game_account_id, faucet_account_id).await;
    fund_account(&mut client, game_account_id, faucet_account_id).await;

    let fungible_asset = FungibleAsset::new(faucet_account_id, BUY_IN_AMOUNT).unwrap();

    let player_account_ids = vec![player1_account_id, player2_account.id()];

    let mut cards: Vec<[Felt; 4]> = vec![];

    for slot_index in 1..2 * player_account_ids.len() + 1 {
        let slot_item = game_account_storage.get_item(slot_index as u8);
        cards.push(slot_item.into());
    }

    println!("Card {:?}", cards);

    println!("Start sending cards to players");
    for (i, _) in player_account_ids.iter().enumerate() {
        let target_account_id = player_account_ids[i];
        println!("Target account id {:?}", target_account_id);

        let input_cards = [cards[i], cards[i + 1]]; // don't you think the input cards should contain 8 felt -> 2 cards
        let sendcard_txn_data = SendCardTransactionData::new(
            Asset::Fungible(fungible_asset),
            game_account_id,
            target_account_id,
            &input_cards
        );

        let transaction_template = AzeTransactionTemplate::SendCard(sendcard_txn_data);

        let txn_request = client.build_aze_send_card_tx_request(transaction_template).unwrap();
        execute_tx_and_sync(&mut client, txn_request.clone()).await;

        let note_id = txn_request.expected_output_notes()[0].id();
        let note = client.get_input_note(note_id).unwrap();

        let tx_template = TransactionTemplate::ConsumeNotes(target_account_id, vec![note.id()]);
        let tx_request = client.build_transaction_request(tx_template).unwrap();
        execute_tx_and_sync(&mut client, tx_request).await;

        println!("Executed and synced with node");
        assert_account_status(&client, target_account_id, i).await;
    }
}

#[tokio::test]
async fn test_play_bet() {
    let mut client: AzeClient = create_test_client();

    let (game_account, player_account_id, faucet_account_id, game_slot_data) = setup_accounts(
        &mut client
    );

    let game_account_storage = game_account.storage();

    let fungible_asset = FungibleAsset::new(faucet_account_id, BUY_IN_AMOUNT).unwrap();

    let sender_account_id = player_account_id;
    let target_account_id = game_account.id();

    fund_account(&mut client, sender_account_id, faucet_account_id).await;

    let player_bet = SMALL_BLIND_AMOUNT;

    let playbet_txn_data = PlayBetTransactionData::new(
        Asset::Fungible(fungible_asset),
        sender_account_id,
        target_account_id,
        player_bet
    );
    let transaction_template = AzeTransactionTemplate::PlayBet(playbet_txn_data);
    let txn_request = client.build_aze_play_bet_tx_request(transaction_template).unwrap();
    execute_tx_and_sync(&mut client, txn_request.clone()).await;

    let note_id = txn_request.expected_output_notes()[0].id();
    let note = client.get_input_note(note_id).unwrap();

    let tx_template = TransactionTemplate::ConsumeNotes(target_account_id, vec![note.id()]);
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client, tx_request).await;

    println!("Executed and synced with node");
    assert_slot_status_bet(&client, target_account_id, game_slot_data).await;
}

#[tokio::test]
async fn test_play_raise() {
    let mut client: AzeClient = create_test_client();

    let (game_account, player_account_id, faucet_account_id, game_slot_data) = setup_accounts(
        &mut client
    );

    let game_account_storage = game_account.storage();

    let fungible_asset = FungibleAsset::new(faucet_account_id, BUY_IN_AMOUNT).unwrap();

    let sender_account_id = player_account_id;
    let target_account_id = game_account.id();

    fund_account(&mut client, sender_account_id, faucet_account_id).await;

    let player_bet = SMALL_BLIND_AMOUNT;

    let playraise_txn_data = PlayRaiseTransactionData::new(
        Asset::Fungible(fungible_asset),
        sender_account_id,
        target_account_id,
        player_bet
    );
    let transaction_template = AzeTransactionTemplate::PlayRaise(playraise_txn_data);
    let txn_request = client.build_aze_play_raise_tx_request(transaction_template).unwrap();
    execute_tx_and_sync(&mut client, txn_request.clone()).await;

    let note_id = txn_request.expected_output_notes()[0].id();
    let note = client.get_input_note(note_id).unwrap();

    let tx_template = TransactionTemplate::ConsumeNotes(target_account_id, vec![note.id()]);
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client, tx_request).await;

    println!("Executed and synced with node");
    assert_slot_status_raise(&client, target_account_id, game_slot_data).await;
}

#[tokio::test]
async fn test_play_call() {
    let mut client: AzeClient = create_test_client();

    let (game_account, player_account_id, faucet_account_id, game_slot_data) = setup_accounts(
        &mut client
    );

    let game_account_storage = game_account.storage();

    let fungible_asset = FungibleAsset::new(faucet_account_id, BUY_IN_AMOUNT).unwrap();

    let sender_account_id = player_account_id;
    let target_account_id = game_account.id();

    fund_account(&mut client, sender_account_id, faucet_account_id).await;

    let playraise_txn_data = PlayCallTransactionData::new(
        Asset::Fungible(fungible_asset),
        sender_account_id,
        target_account_id
    );

    let transaction_template = AzeTransactionTemplate::PlayCall(playraise_txn_data);
    let txn_request = client.build_aze_play_call_tx_request(transaction_template).unwrap();
    execute_tx_and_sync(&mut client, txn_request.clone()).await;

    let note_id = txn_request.expected_output_notes()[0].id();
    let note = client.get_input_note(note_id).unwrap();

    let tx_template = TransactionTemplate::ConsumeNotes(target_account_id, vec![note.id()]);
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client, tx_request).await;

    println!("Executed and synced with node");
    assert_slot_status_call(&client, target_account_id, game_slot_data).await;
}

#[tokio::test]
async fn test_play_fold() {
    let mut client: AzeClient = create_test_client();

    let (game_account, player_account_id, faucet_account_id, game_slot_data) = setup_accounts(
        &mut client
    );

    let game_account_storage = game_account.storage();

    let fungible_asset = FungibleAsset::new(faucet_account_id, BUY_IN_AMOUNT).unwrap();

    let sender_account_id = player_account_id;
    let target_account_id = game_account.id();

    fund_account(&mut client, sender_account_id, faucet_account_id).await;

    let playfold_txn_data = PlayFoldTransactionData::new(
        Asset::Fungible(fungible_asset),
        sender_account_id,
        target_account_id
    );

    let transaction_template = AzeTransactionTemplate::PlayFold(playfold_txn_data);
    let txn_request = client.build_aze_play_fold_tx_request(transaction_template).unwrap();
    execute_tx_and_sync(&mut client, txn_request.clone()).await;

    let note_id = txn_request.expected_output_notes()[0].id();
    let note = client.get_input_note(note_id).unwrap();

    let tx_template = TransactionTemplate::ConsumeNotes(target_account_id, vec![note.id()]);
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client, tx_request).await;

    println!("Executed and synced with node");
    assert_slot_status_fold(&client, target_account_id, game_slot_data).await;
}

#[tokio::test]
async fn test_play_check() {
    let mut client: AzeClient = create_test_client();

    let (game_account, player_account_id, faucet_account_id, game_slot_data) = setup_accounts(
        &mut client
    );

    let game_account_storage = game_account.storage();

    let fungible_asset = FungibleAsset::new(faucet_account_id, BUY_IN_AMOUNT).unwrap();
    let sender_account_id = player_account_id;
    let target_account_id = game_account.id();

    fund_account(&mut client, sender_account_id, faucet_account_id).await;

    let playcheck_txn_data = PlayCheckTransactionData::new(
        Asset::Fungible(fungible_asset),
        sender_account_id,
        target_account_id
    );

    let transaction_template = AzeTransactionTemplate::PlayCheck(playcheck_txn_data);
    let txn_request = client.build_aze_play_check_tx_request(transaction_template).unwrap();
    execute_tx_and_sync(&mut client, txn_request.clone()).await;

    let note_id = txn_request.expected_output_notes()[0].id();
    let note = client.get_input_note(note_id).unwrap();

    let tx_template = TransactionTemplate::ConsumeNotes(target_account_id, vec![note.id()]);
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client, tx_request).await;

    println!("Executed and synced with node");
    assert_slot_status_check(&client, target_account_id, game_slot_data.clone(), 1 as u8).await;
}

async fn assert_account_status(client: &AzeClient, account_id: AccountId, index: usize) {
    let (account, _) = client.get_account(account_id).unwrap();
    let card_suit = 1u8;

    assert_eq!(account.vault().assets().count(), 1);
    assert_eq!(
        account.storage().get_item(100),
        RpoDigest::new([
            Felt::from(card_suit),
            Felt::from((index + 1) as u8),
            Felt::ZERO,
            Felt::ZERO,
        ])
    );
    assert_eq!(
        account.storage().get_item(101),
        RpoDigest::new([
            Felt::from(card_suit),
            Felt::from((index + 2) as u8),
            Felt::ZERO,
            Felt::ZERO,
        ])
    );
}

async fn assert_slot_status_bet(
    client: &AzeClient,
    account_id: AccountId,
    slot_data: GameStorageSlotData
) {
    let (account, _) = client.get_account(account_id).unwrap();
    let game_account_storage = account.storage();

    let player_index = slot_data.current_turn_index();
    let player_bet = SMALL_BLIND_AMOUNT;

    // check highest bet
    assert_eq!(
        game_account_storage.get_item(HIGHEST_BET_SLOT),
        RpoDigest::new([Felt::from(player_bet), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );
    // check player bet
    assert_eq!(
        game_account_storage.get_item((player_index + PLAYER_BET_OFFSET) as u8),
        RpoDigest::new([Felt::from(player_bet), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );
    // check current player index
    assert_eq!(
        game_account_storage.get_item(CURRENT_TURN_INDEX_SLOT),
        RpoDigest::new([
            Felt::from(player_index + PLAYER_STATS_SLOTS),
            Felt::ZERO,
            Felt::ZERO,
            Felt::ZERO,
        ])
    );
}

async fn assert_slot_status_raise(
    client: &AzeClient,
    account_id: AccountId,
    slot_data: GameStorageSlotData
) {
    let (account, _) = client.get_account(account_id).unwrap();
    let game_account_storage = account.storage();

    let small_blind_amt = slot_data.small_blind_amt();
    let buy_in_amt = slot_data.buy_in_amt();
    let no_of_players = slot_data.player_count();
    let flop_index = slot_data.flop_index();

    let mut slot_index = 1;

    // check are the cards has been correctly placed
    for card_suit in 1..5 {
        for card_number in 1..14 {
            let slot_item = RpoDigest::new([
                Felt::from(card_suit as u8),
                Felt::from(card_number as u8),
                Felt::ZERO, // denotes is encrypted
                Felt::ZERO,
            ]);

            assert_eq!(game_account_storage.get_item(slot_index), slot_item);

            slot_index = slot_index + 1;
        }
    }

    // checking next turn
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(flop_index as u8), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;
    // checking the small blind amount
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(small_blind_amt), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;
    // checking the big blind amount
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(small_blind_amt * 2), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;
    // checking the buy in amount
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(buy_in_amt), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;
    // checking no of player slot
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(no_of_players), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    slot_index = slot_index + 1;
    // checking raiser
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([
            Felt::from(slot_data.current_turn_index()),
            Felt::ZERO,
            Felt::ZERO,
            Felt::ZERO,
        ])
    );

    slot_index = slot_index + 2;
    // check current player index
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([
            Felt::from(slot_data.current_turn_index() + PLAYER_STATS_SLOTS),
            Felt::ZERO,
            Felt::ZERO,
            Felt::ZERO,
        ])
    );

    slot_index = slot_index + 1;
    // check highest bet
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(slot_data.highest_bet()), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    let player_bet = SMALL_BLIND_AMOUNT;
    slot_index = slot_index + 6;
    // check player bet
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(player_bet), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    let remaining_balance = slot_data.player_balance() - player_bet;
    slot_index = slot_index + 1;
    // check player balance
    assert_eq!(
        game_account_storage.get_item(slot_index),
        RpoDigest::new([Felt::from(remaining_balance), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );
}

async fn assert_slot_status_call(
    client: &AzeClient,
    account_id: AccountId,
    slot_data: GameStorageSlotData
) {
    let (account, _) = client.get_account(account_id).unwrap();
    let game_account_storage = account.storage();

    let remaining_balance = slot_data.player_balance() - slot_data.highest_bet();

    // check player balance
    assert_eq!(
        game_account_storage.get_item(PLAYER_BALANCE_SLOT),
        RpoDigest::new([Felt::from(remaining_balance), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );
}

async fn assert_slot_status_fold(
    client: &AzeClient,
    account_id: AccountId,
    slot_data: GameStorageSlotData
) {
    let (account, _) = client.get_account(account_id).unwrap();
    let game_account_storage = account.storage();

    let fold_index = slot_data.current_turn_index() + IS_FOLD_OFFSET;

    // check is_fold
    assert_eq!(
        game_account_storage.get_item(fold_index),
        RpoDigest::new([Felt::from(1 as u8), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );

    let next_turn_index = slot_data.current_turn_index() + PLAYER_STATS_SLOTS;
    // check next turn index
    assert_eq!(
        game_account_storage.get_item(CURRENT_TURN_INDEX_SLOT),
        RpoDigest::new([Felt::from(next_turn_index), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );
}

async fn assert_slot_status_check(
    client: &AzeClient,
    account_id: AccountId,
    slot_data: GameStorageSlotData,
    player_number: u8
) {
    let (account, _) = client.get_account(account_id).unwrap();
    let game_account_storage = account.storage();

    // assert check count
    let check_count = game_account_storage.get_item(CHECK_COUNTER_SLOT);
    assert_eq!(check_count, RpoDigest::new([Felt::from(player_number as u8), Felt::ZERO, Felt::ZERO, Felt::ZERO]));

    let next_turn_index = slot_data.current_turn_index() + PLAYER_STATS_SLOTS * player_number;
    // check next turn index
    assert_eq!(
        game_account_storage.get_item(CURRENT_TURN_INDEX_SLOT),
        RpoDigest::new([Felt::from(next_turn_index), Felt::ZERO, Felt::ZERO, Felt::ZERO])
    );
}

async fn fund_account(client: &mut AzeClient, account_id: AccountId, faucet_account_id: AccountId) {
    let note = mint_note(client, account_id, faucet_account_id, NoteType::Public).await;
    consume_notes(client, account_id, &[note]).await;
}