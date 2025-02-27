use hellas_consensus::morpheus::utils::TestHelper;

fn main() {
    println!("Running Morpheus consensus tests");
    
    // Create a test network with 4 nodes
    let mut simulator = TestHelper::create_test_network(
        4,              // 4 nodes
        50,             // 50ms base network delay
        vec![],         // No Byzantine nodes
    );
    
    // Run a simple test
    TestHelper::run_simple_test(
        &mut simulator,
        5,              // 5 transactions per node
        5,              // Run for 5 seconds
    );
    
    // Create another network for partition test
    let mut simulator = TestHelper::create_test_network(
        4,              // 4 nodes
        50,             // 50ms base network delay
        vec![],         // No Byzantine nodes
    );
    
    // Run a network partition test
    TestHelper::run_partition_test(
        &mut simulator,
        5,              // 5 transactions per node
        1,              // 1 second before partition
        2,              // 2 seconds of partition
        5,              // 5 seconds total runtime
    );
    
    println!("All tests completed");
}