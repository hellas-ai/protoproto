fn main() {
    let kp = libp2p::identity::Keypair::generate_ed25519();
    let edkp = kp.try_into_ed25519().unwrap();
    println!("private: {}", hex::encode(edkp.secret().as_ref()));
    println!("public: {}", hex::encode(edkp.public().to_bytes()));
}
