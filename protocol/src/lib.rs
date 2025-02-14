pub enum Transaction { 
    Increment,
    Decrement
}

pub struct Block {
    pub txns: Vec<Transaction>,
    pub sig: Signature,
}
