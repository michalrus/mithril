//! Multisignature scheme API

use super::Index;

use blstrs::{pairing, Scalar, Field, G1Affine, G1Projective, G2Projective, G2Affine};
use rand_core::OsRng;
use groupy::CurveAffine;
use blake2::VarBlake2b;
use digest::{VariableOutput,Update};

pub struct MSP { }

#[derive(Clone,Copy)]
pub struct SK(Scalar);

#[derive(Clone,Copy)]
pub struct MVK(pub G2Projective);

#[derive(Clone,Copy)]
pub struct PK {
    pub mvk: MVK,
    pub k1: G1Projective,
    pub k2: G1Projective,
}

#[derive(Clone,Copy)]
pub struct Sig(G1Projective);

static POP: &[u8] = b"PoP";
static M: &[u8]   = b"M";

impl MSP {
    pub fn gen() -> (SK, PK) {
        // sk=x <- Zq
        let mut rng = OsRng::default();
        let x = Scalar::random(&mut rng);
        // mvk <- g2^x
        let mvk = MVK(G2Affine::one() * x);
        // k1 <- H_G1("PoP"||mvk)^x
        let k1 = hash_to_g1(POP, &mvk.to_bytes()) * x;
        // k2 <- g1^x
        let k2 = G1Affine::one() * x;
        // return sk,mvk,k=(k1,k2)
        (SK(x), PK { mvk, k1, k2 })

    }

    pub fn check(pk: &PK) -> bool {
        // if e(k1,g2) = e(H_G1("PoP"||mvk),mvk)
        //      and e(g1,mvk) = e(k2,g2)
        //      are both true, return 1
        let mvk_g2 = G2Affine::from(pk.mvk.0);
        let e_k1_g2   = pairing(pk.k1.into(), G2Affine::one());
        let h_pop_mvk = hash_to_g1(POP, &pk.mvk.to_bytes());
        let e_hg1_mvk = pairing(h_pop_mvk, mvk_g2);

        let e_g1_mvk = pairing(G1Affine::one(), mvk_g2);
        let e_k2_g2  = pairing(pk.k2.into(), G2Affine::one());

        (e_k1_g2 == e_hg1_mvk) && (e_g1_mvk == e_k2_g2)
    }

    pub fn sig(sk: &SK, msg: &[u8]) -> Sig {
        // return sigma <- H_G1("M"||msg)^x
        let g1 = hash_to_g1(M, msg);
        Sig(g1 * sk.0)
    }

    pub fn ver(msg: &[u8], mvk: &MVK, sigma: &Sig) -> bool {
        // return 1 if e(sigma,g2) = e(H_G1("M"||msg),mvk)
        let e_sigma_g2 = pairing(G1Affine::from(sigma.0), G2Affine::one());
        let e_hg1_mvk  = pairing(hash_to_g1(M, msg), G2Affine::from(mvk.0));

        e_sigma_g2 == e_hg1_mvk
    }

    // MSP.AKey
    pub fn aggregate_keys(mvks: &[MVK]) -> MVK {
        MVK(mvks
            .iter()
            .fold(G2Projective::from(G2Affine::zero()),
                  |acc, x| acc + x.0))
    }

    // MSP.Aggr
    pub fn aggregate_sigs(msg: &[u8], sigmas: &[Sig]) -> Sig {
        // XXX: what is d?
        Sig(sigmas
            .iter()
            .fold(G1Projective::from(G1Affine::zero()),
                  |acc, s| acc + s.0))
    }

    // MSP.AVer
    pub fn aggregate_ver(msg: &[u8], ivk: &MVK, mu: &Sig) -> bool {
        Self::ver(msg, ivk, mu)
    }

    pub fn eval(msg: &[u8], index: Index, sigma: &Sig) -> u64 {
        let mut hasher : VarBlake2b = VariableOutput::new(8).unwrap();
        // H("map"||msg||index||sigma)
        hasher.update(&["map".as_bytes(),
                        msg,
                        &index.0.to_le_bytes(),
                        &sigma.0.to_uncompressed()].concat());
        let mut dest = [0 as u8; 8];
        hasher.finalize_variable(|out| {
            dest.copy_from_slice(out);
        });
        u64::from_le_bytes(dest)
        // XXX: See section 6 to implement M from Elligator Squared
        // return ev <- M_msg,index(sigma)
    }
}

impl MVK {
    pub fn to_bytes(&self) -> [u8; 96] {
        // Notes: to_vec() here causes a segfault later, why?
        self.0.to_uncompressed()
    }
}

fn hash_to_g1(tag: &[u8], bytes: &[u8]) -> G1Affine {
    // k1 <- H_G1("PoP"||mvk)^x
    G1Affine::from(G1Projective::hash_to_curve(bytes, b"mithril", tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_pair_prop(x in any::<u64>(),
                          y in any::<u64>()) {
            // Sanity check that the library behaves as expected
            let sx = blstrs::Scalar::from(x);
            let sy = blstrs::Scalar::from(y);
            let gt = pairing((G1Affine::one() * sx).into(),
                             (G2Affine::one() * sy).into());
            let should_be = pairing((G1Affine::one() * (sx * sy)).into(), G2Affine::one());
            assert!(gt == should_be);
        }

        #[test]
        fn test_sig(msg in prop::collection::vec(any::<u8>(), 1..128)) {
            let (sk, pk) = MSP::gen();
            let sig = MSP::sig(&sk, &msg);
            assert!(MSP::ver(&msg, &pk.mvk, &sig));
        }

        #[test]
        fn test_aggregate_sig(msg in prop::collection::vec(any::<u8>(), 1..128),
                              num_sigs in 1..16) {
            let mut mvks = Vec::new();
            let mut sigs = Vec::new();
            for _ in 0..num_sigs {
                let (sk, pk) = MSP::gen();
                let sig = MSP::sig(&sk, &msg);
                assert!(MSP::ver(&msg, &pk.mvk, &sig));
                sigs.push(sig);
                mvks.push(pk.mvk);
            }
            let ivk = MSP::aggregate_keys(&mvks);
            let mu = MSP::aggregate_sigs(&msg, &sigs);
            assert!(MSP::aggregate_ver(&msg, &ivk, &mu));
        }

        #[test]
        fn test_eval_sanity_check(msg in prop::collection::vec(any::<u8>(), 1..128),
                                  idx in any::<u64>(),
                                  s in any::<u64>()) {
            let sigma = Sig(G1Affine::one() * blstrs::Scalar::from(s));
            MSP::eval(&msg, Index(idx), &sigma);
        }
    }

    #[test]
    fn test_gen() {
        for _ in 0..128 {
            let (_sk, pk) = MSP::gen();
            assert!(MSP::check(&pk));
        }
    }
}
