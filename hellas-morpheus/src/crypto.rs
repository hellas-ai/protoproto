use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Valid};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A unique identifier for a process
#[derive(
    PartialEq,
    Clone,
    PartialOrd,
    Eq,
    Hash,
    Ord,
    Debug,
    Serialize,
    Default,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
    Copy
)]
pub struct Identity(pub u32);

/// Collects the public keys of all identities.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Default)]
pub struct KeyBook {
    pub keys: BTreeMap<Identity, hints::PublicKey>,
    pub identities: BTreeMap<hints::PublicKey, Identity>,
    pub me_identity: Identity,
    pub me_pub_key: hints::PublicKey,
    pub me_sec_key: hints::SecretKey,
    pub hints_setup: Option<hints::UniverseSetup>,
}

#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
    derivative::Derivative,
)]
#[derivative(Debug)]
pub struct Signed<T: Valid + CanonicalSerialize + CanonicalDeserialize> {
    pub data: T,
    pub author: Identity,
    // eventually: replace with some other faster signature scheme
    #[derivative(Debug = "ignore")]
    pub signature: hints::PartialSignature,
}

#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
    Derivative,
)]
#[derivative(Debug)]
pub struct ThreshSigned<T: Valid + CanonicalSerialize + CanonicalDeserialize> {
    pub data: T,
    #[derivative(Debug = "ignore")]
    pub signature: hints::Signature,
}

#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
    Derivative,
)]
#[derivative(Debug)]
pub struct ThreshPartial<T: Valid + CanonicalSerialize + CanonicalDeserialize> {
    pub data: T,
    pub author: Identity,
    #[derivative(Debug = "ignore")]
    pub signature: hints::PartialSignature,
}

impl<T: CanonicalSerialize + CanonicalDeserialize> ThreshSigned<T> {
    #[cfg(not(test))]
    pub fn valid_signature(&self, keybook: &KeyBook, threshold: u32) -> bool {
        let verifier = keybook.hints_setup.as_ref().unwrap().verifier();
        let mut buf = Vec::new();
        T::serialize_compressed(&self.data, &mut buf).unwrap();
        hints::verify_aggregate(&verifier, &self.signature, &buf).is_ok()
            && self.signature.threshold >= hints::F::from(threshold)
    }

    #[cfg(test)]
    pub fn valid_signature(&self, keybook: &KeyBook, threshold: u32) -> bool {
        true
    }
}

impl<T: CanonicalSerialize + CanonicalDeserialize> ThreshPartial<T> {
    pub fn from_data(data: T, kb: &KeyBook) -> Self {
        let mut buf = Vec::new();
        T::serialize_compressed(&data, &mut buf).unwrap();
        let sig = hints::sign(&kb.me_sec_key, &buf);
        Self {
            data,
            author: kb.me_identity.clone(),
            signature: sig,
        }
    }

    #[cfg(not(test))]
    pub fn valid_signature(&self, keybook: &KeyBook) -> bool {
        let their_key = keybook
            .keys
            .get(&self.author)
            .expect("author not in keybook");
        let mut buf = Vec::new();
        T::serialize_compressed(&self.data, &mut buf).unwrap();
        hints::verify_partial(
            &keybook.hints_setup.as_ref().unwrap().global,
            their_key,
            &buf,
            &self.signature,
        )
    }

    #[cfg(test)]
    pub fn valid_signature(&self, keybook: &KeyBook) -> bool {
        true
    }
}

impl<T: CanonicalSerialize + CanonicalDeserialize> Signed<T> {
    pub fn from_data(data: T, kb: &KeyBook) -> Self {
        let mut buf = Vec::new();
        T::serialize_compressed(&data, &mut buf).unwrap();
        let sig = hints::sign(&kb.me_sec_key, &buf);
        Self {
            data,
            author: kb.me_identity.clone(),
            signature: sig,
        }
    }

    #[cfg(not(test))]
    pub fn valid_signature(&self, keybook: &KeyBook) -> bool {
        let their_key = keybook
            .keys
            .get(&self.author)
            .expect("author not in keybook");
        let mut buf = Vec::new();
        T::serialize_compressed(&self.data, &mut buf).unwrap();
        hints::verify_partial(
            &keybook.hints_setup.as_ref().unwrap().global,
            their_key,
            &buf,
            &self.signature,
        )
    }

    #[cfg(test)]
    pub fn valid_signature(&self, keybook: &KeyBook) -> bool {
        true
    }
}
