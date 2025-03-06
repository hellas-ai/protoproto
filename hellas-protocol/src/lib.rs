mod provider;
mod auditor;
mod requestor;

pub struct Signature;
pub struct Pubkey;
pub struct TokenAmount(pub u64);


pub struct Signed<T> {
    pub data: T,
    pub signature: Signature,
}

pub struct BFJob {
    pub program: String
}

pub struct ExecutionPolicy {
    pub invalidity: Option<Collateral>,
    pub timeout: Option<TimeoutConfig>,
}

pub struct TimeoutConfig {
    pub timeout: u64,
    pub penalty: Collateral,
}

pub enum Collateral {
    None,
    BurnPerformanceBond { amount: u64 }
}

pub struct QuoteRequest  {
    pub job: BFJob,
    pub policy: ExecutionPolicy,
}

pub struct JobQuote {
    pub requested: QuoteRequest,
    pub price: u64,
}

pub struct AcceptedJobQuote {
    pub quote: JobQuote,
    pub provider: Pubkey,
    pub requestor: Pubkey,
    pub provider_signature: Signature,
    pub requestor_signature: Signature,
}

pub enum Transaction { 
    Increment,
    Decrement
}

pub struct Block {
    pub txns: Vec<Transaction>,
}