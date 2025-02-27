mod provider;
mod auditor;
mod requestor;

pub struct Signature;
pub struct Pubkey;

pub struct Signed<T> {
    pub data: T,
    pub signature: Signature,
}

pub struct BFJob {
    pub program: String
}

pub enum SecurityPolicy {
    None,
    BurnPerformanceBond { amount: u64 }
}

pub struct QuoteRequest  {
    pub job: BFJob,
    pub security_policy: SecurityPolicy,
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