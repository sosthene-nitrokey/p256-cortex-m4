use core::convert::TryInto;

use rand_core::{CryptoRng, RngCore};

use crate::{Error, Result};

impl From<elliptic_curve::Error> for Error {
    fn from(_: elliptic_curve::Error) -> Self {
        Self
    }
}

impl From<p256::ecdsa::Error> for Error {
    fn from(_: p256::ecdsa::Error) -> Self {
        Self
    }
}

/// NIST P-256 secret key.
#[derive(Clone)]
pub struct SecretKey(p256::SecretKey);

/// NIST P-256 public key.
#[derive(Clone, Debug)]
pub struct PublicKey(p256::PublicKey);

/// NIST P-256 keypair.
#[derive(Clone)]
pub struct Keypair {
    /// Public key of the keypair
    pub public: PublicKey,
    /// Secret key of the keypair
    pub secret: SecretKey,
}

/// NIST P-256 signature.
#[derive(Clone, Debug)]
pub struct Signature(p256::ecdsa::Signature);

/// Outcome of ECDH key agreement.
pub struct SharedSecret(p256::ecdh::SharedSecret);

impl Keypair {
    /// Generate a random `Keypair`.
    ///
    /// The implementation uses rejection sampling.
    pub fn random(rng: impl CryptoRng + RngCore) -> Self {
        let secret = SecretKey(p256::SecretKey::random(rng));
        let public = secret.public_key();

        Keypair { public, secret }
    }
}

impl SecretKey {
    /// Generate a random `SecretKey`.
    ///
    /// The implementation uses rejection sampling.
    pub fn random(rng: impl CryptoRng + RngCore) -> Self {
        SecretKey(p256::SecretKey::random(rng))
    }

    /// Verifies that there are 32 bytes that correspond to a big-endian integer in the range 1..=n-1.
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Result<Self> {
        Ok(SecretKey(p256::SecretKey::from_bytes(bytes)?))
    }

    /// Return secret scalar as big-endian integer.
    pub unsafe fn to_bytes(&self) -> [u8; 32] {
        let mut big_endian = [0u8; 32];
        big_endian.copy_from_slice(&self.0.to_bytes());
        big_endian
    }

    /// Calculate associated public key.
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.public_key())
    }

    /// Attempt at unraveling the traits in `p256`.
    pub fn sign_prehashed(&self, prehashed_message: &[u8], rng: impl CryptoRng + RngCore) -> Signature {
        // use p256::ecdsa::signature::Signer;
        // let signature = signer.sign(message);
        // signature

        let prehashed_message_as_scalar = p256::Scalar::from_bytes_reduced(prehashed_message.try_into().unwrap());
        let mut rng = rng;
        let static_scalar = p256::Scalar::from_bytes_reduced(&self.0.to_bytes());
        loop {
            let ephemeral_secret = p256::SecretKey::random(&mut rng);
            let ephemeral_scalar = p256::Scalar::from_bytes_reduced(&ephemeral_secret.to_bytes());
            let blinded_scalar = p256::BlindedScalar::new(ephemeral_scalar, &mut rng);

            use ecdsa::hazmat::SignPrimitive;
            if let Ok(signature) = static_scalar.try_sign_prehashed(
                &blinded_scalar,
                &prehashed_message_as_scalar,
            ) {
                return Signature(signature);
            }
        }
    }

    #[cfg(feature = "prehash")]
    #[cfg_attr(docsrs, doc(cfg(feature = "prehash")))]
    /// Deterministic signature on message, which is hashed with SHA-256 first.
    pub fn sign(&self, message: &[u8], _rng: impl CryptoRng + RngCore) -> Signature {
        let signer: p256::ecdsa::SigningKey = self.0.clone().into();
        use p256::ecdsa::signature::Signer;
        let signature = signer.sign(message);
        Signature(signature)
    }

    /// ECDH key agreement.
    pub fn agree(&self, other: &PublicKey) -> SharedSecret {

        let shared_secret = elliptic_curve::ecdh::diffie_hellman(
            self.0.secret_scalar(),
            other.0.as_affine(),
        );

        SharedSecret(shared_secret)
    }
}

impl PublicKey {
    /// Decode `PublicKey` (compressed or uncompressed) from the
    /// `Elliptic-Curve-Point-to-Octet-String` encoding in [SEC 1][sec-1] (section 2.3.3)
    ///
   /// This is the left-inverse of both `to_compressed_bytes` and `to_uncompressed_bytes`.
   ///
   /// [sec-1]: http://www.secg.org/sec1-v2.pdf
   pub fn from_sec1_bytes(bytes: &[u8]) -> Result<Self> {
       Ok(PublicKey(p256::PublicKey::from_sec1_bytes(bytes)?))
   }

   /// Decode assuming `bytes` is x-coordinate then y-coordinate, both big-endian 32B arrays.
   ///
   /// In other words, the uncompressed SEC1 format, without the leading 0x04 byte.
   pub fn from_untagged_bytes(bytes: &[u8]) -> Result<Self> {
       if bytes.len() != 64 {
           return Err(Error);
       }
       let mut sec1_bytes = [4u8; 65];
       sec1_bytes[1..].copy_from_slice(bytes);
       Self::from_sec1_bytes(&sec1_bytes)
   }

   /// Compressed encoding: `02 || Px` if Py is even and `03 || Px` if Py is odd
   pub fn to_compressed_bytes(&self) -> [u8; 33] {
       use elliptic_curve::sec1::ToEncodedPoint;
       let encoded_point = self.0.to_encoded_point(true);
       let mut bytes = [0u8; 33];
       bytes.copy_from_slice(encoded_point.as_bytes());
       bytes
   }

   /// Uncompressed encoding: `04 || Px || Py`.
   pub fn to_uncompressed_bytes(&self) -> [u8; 65] {
       use elliptic_curve::sec1::ToEncodedPoint;
       let encoded_point = self.0.to_encoded_point(false);
       let mut bytes = [0u8; 65];
       bytes.copy_from_slice(encoded_point.as_bytes());
       bytes
   }

   /// Big-endian representation of x-coordinate.
   pub fn x(&self) -> [u8; 32] {
       self.to_uncompressed_bytes()[1..33].try_into().unwrap()
   }

   /// Big-endian representation of x-coordinate.
   pub fn y(&self) -> [u8; 32] {
       self.to_uncompressed_bytes()[33..].try_into().unwrap()
   }

   /// Verify signature on message assumed to be hashed, if needed.
   pub fn verify_prehashed(&self, prehashed_message: &[u8], signature: &Signature) -> bool {
       let prehashed_message_as_scalar = p256::Scalar::from_bytes_reduced(prehashed_message.try_into().unwrap());
       use ecdsa::hazmat::VerifyPrimitive;

       self.0.as_affine().verify_prehashed(&prehashed_message_as_scalar, &signature.0).is_ok()
   }

   /// Verify signature on message, which is hashed with SHA-256 first.
   #[cfg(feature = "prehash")]
   #[cfg_attr(docsrs, doc(cfg(feature = "prehash")))]
   pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
       let verifier: p256::ecdsa::VerifyingKey = self.0.clone().into();
       use p256::ecdsa::signature::Verifier;
       verifier.verify(message, &signature.0).is_ok()
   }

}

impl Signature {
    // /// Big-endian representation of r.
    // pub fn r(&self) -> [u8; 32] {
    //     let mut r = [0u8; 32];
    //     unsafe { p256_cortex_m4_sys::p256_convert_endianness(
    //         &mut r[0] as *mut u8 as *mut _,
    //         &self.r[0] as *const u32 as *const _,
    //         32,
    //     ) };
    //     r
    // }

    // /// Big-endian representation of s.
    // pub fn s(&self) -> [u8; 32] {
    //     let mut s = [0u8; 32];
    //     unsafe { p256_cortex_m4_sys::p256_convert_endianness(
    //         &mut s[0] as *mut u8 as *mut _,
    //         &self.s[0] as *const u32 as *const _,
    //         32,
    //     ) };
    //     s
    // }

   /// Decode signature as big-endian r, then big-endian s, without framing.
   ///
   /// Necessarily, bytes must be of length 64, and r and s must be integers
   /// in the range 1..=n-1, otherwise decoding fails.
   pub fn try_from_bytes(bytes: &[u8]) -> Result<Self> {
       Ok(Signature(bytes.try_into()?))
   }

   /// Encode signature from big-endian r, then big-endian s, without framing.
   pub fn to_bytes(&self) -> [u8; 64] {
       let mut bytes = [0u8; 64];
       bytes.copy_from_slice(self.0.as_ref());
       bytes
   }

   /// Decode signature from ASN.1 DER
   pub fn from_asn1(bytes: &[u8]) -> Result<Self> {
       Ok(Signature(p256::ecdsa::Signature::from_asn1(bytes)?))
   }

   /// Encode signature as ASN.1 DER
   ///
   /// This means interpreting signature as a SEQUENCE of (unsigned) INTEGERs.
   #[cfg(feature = "der-signatures")]
   #[cfg_attr(docsrs, doc(cfg(feature = "der-signatures")))]
   pub fn to_der(&self, buffer: &mut [u8; 72]) -> usize {
       let asn1_signature = self.0.to_asn1();
       buffer.copy_from_slice(asn1_signature.as_ref());
       asn1_signature.as_ref().len()
   }
}

impl SharedSecret {
    /// The secret (big-endian x-coordinate)
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes().as_ref()
    }
}