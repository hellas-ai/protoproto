use hellas_morpheus::{
    Block, BlockData, BlockHash, BlockKey, BlockType, Identity, Message, Phase, Signed, SlotNum,
    StartView, ThreshPartial, ThreshSigned, Transaction, ViewNum, VoteData,
    test_harness::TestTransaction,
};
use std::sync::Arc;

#[test_log::test]
fn test_format_functions() {
    // Create some test instances
    let identity = Identity(42);
    let view_num = ViewNum(5);
    let slot_num = SlotNum(3);
    let block_hash = BlockHash(0xCAFEBABE);

    // Create a vote data
    let block_key = BlockKey {
        type_: BlockType::Tr,
        view: view_num,
        height: 10,
        author: Some(identity.clone()),
        slot: slot_num,
        hash: Some(block_hash.clone()),
    };

    let vote_data = VoteData {
        z: 1,
        for_which: block_key.clone(),
    };

    let signed_vote = ThreshPartial {
        data: vote_data.clone(),
        author: identity.clone(),
        signature: hints::PartialSignature::default(),
    };

    let thresh_signed_vote = Arc::new(ThreshSigned {
        data: vote_data.clone(),
        signature: hints::Signature::default(),
    });

    // Create a block
    let block = Block {
        key: block_key.clone(),
        prev: vec![thresh_signed_vote.clone()],
        one: thresh_signed_vote.clone(),
        data: BlockData::Tr {
            transactions: vec![TestTransaction(vec![1, 2, 3, 4])],
        },
    };

    let signed_block = Arc::new(Signed {
        data: block.clone(),
        author: identity.clone(),
        signature: hints::PartialSignature::default(),
    });

    // Create various messages
    let messages = vec![
        Message::Block(signed_block.clone()),
        Message::NewVote(Arc::new(signed_vote.clone())),
        Message::QC(thresh_signed_vote.clone()),
        Message::EndView(Arc::new(ThreshPartial {
            data: view_num,
            author: identity.clone(),
            signature: hints::PartialSignature::default(),
        })),
        Message::EndViewCert(Arc::new(ThreshSigned {
            data: view_num,
            signature: hints::Signature::default(),
        })),
        Message::StartView(Arc::new(Signed {
            data: StartView {
                view: view_num,
                qc: thresh_signed_vote.clone(),
            },
            author: identity.clone(),
            signature: hints::PartialSignature::default(),
        })),
    ];

    // Import the formatting functions
    use hellas_morpheus::format::*;

    // Print everything with our custom formatters
    println!("\n==== Basic Types ====");
    println!("Identity: {}", format_identity(&identity));
    println!("ViewNum: {}", format_view_num(&view_num));
    println!("SlotNum: {}", format_slot_num(&slot_num));
    println!("BlockHash: {}", format_block_hash(&block_hash));
    println!(
        "BlockType::Genesis: {}",
        format_block_type(&BlockType::Genesis)
    );
    println!("BlockType::Lead: {}", format_block_type(&BlockType::Lead));
    println!("BlockType::Tr: {}", format_block_type(&BlockType::Tr));
    println!("Phase::High: {}", format_phase(&Phase::High));
    println!("Phase::Low: {}", format_phase(&Phase::Low));

    println!("\n==== Complex Types ====");
    println!("BlockKey: {}", format_block_key(&block_key));
    println!("VoteData: {}", format_vote_data(&vote_data, false));
    println!(
        "Signed<VoteData>: {}",
        format_thresh_partial(&signed_vote, |vd| format_vote_data(vd, false), false)
    );
    println!(
        "ThreshSigned<VoteData>: {}",
        format_thresh_signed(&thresh_signed_vote, |vd| format_vote_data(vd, false), false)
    );

    println!("\n==== Block Types ====");
    println!("Block: {}", format_block(&block, false));
    println!(
        "Signed<Block>: {}",
        format_signed(&signed_block, |b| format_block(b, false), false)
    );

    println!("\n==== Messages ====");
    for (i, msg) in messages.iter().enumerate() {
        println!("Message {}: {}", i, format_message(msg, false));
    }

    println!("\n==== Verbose Format ====");
    println!("VoteData (verbose): {}", format_vote_data(&vote_data, true));
    println!("Block (verbose): {}", format_block(&block, true));
    println!("Message (verbose): {}", format_message(&messages[0], true));

    // Assert that our format functions work
    let vote_format = format_vote_data(&vote_data, false);
    assert!(vote_format.contains("1-"));
    assert!(vote_format.contains("Tr["));

    let block_format = format_block(&block, false);
    assert!(block_format.contains("BlockTr["));
    assert!(block_format.contains("prev:1"));

    // Demo the logging macros
    println!("\n==== Logging Macros Demo ====");

    // General protocol log
    hellas_morpheus::protocol_log!("Processing view change to {}", format_view_num(&view_num));

    // Block log
    hellas_morpheus::block_log!(&block);
    hellas_morpheus::block_log!(&block, true); // Verbose

    // Vote log
    hellas_morpheus::vote_log!(&vote_data);

    // QC log
    hellas_morpheus::qc_log!(&thresh_signed_vote);

    // Message log
    for msg in &messages {
        hellas_morpheus::message_log!(msg);
    }
    hellas_morpheus::message_log!(&messages[0], true); // Verbose
}
