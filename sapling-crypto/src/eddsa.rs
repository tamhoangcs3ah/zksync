//! This is an implementation of EdDSA as refered in literature
//! Generation of randomness is not specified

use ff::{Field, PrimeField, PrimeFieldRepr, BitIterator};
use rand::{Rng, Rand};
use std::io::{self, Read, Write};

use jubjub::{FixedGenerators, JubjubEngine, JubjubParams, Unknown, edwards::Point};
use util::{hash_to_scalar_s};

use blake2_rfc::{blake2s};

fn read_scalar<E: JubjubEngine, R: Read>(reader: R) -> io::Result<E::Fs> {
    let mut s_repr = <E::Fs as PrimeField>::Repr::default();
    s_repr.read_le(reader)?;

    match E::Fs::from_repr(s_repr) {
        Ok(s) => Ok(s),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "scalar is not in field",
        )),
    }
}

fn write_scalar<E: JubjubEngine, W: Write>(s: &E::Fs, writer: W) -> io::Result<()> {
    s.into_repr().write_le(writer)
}

fn h_star_s<E: JubjubEngine>(a: &[u8], b: &[u8]) -> E::Fs {
    let personalization_bytes: &[u8] = &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
    hash_to_scalar_s::<E>(personalization_bytes, a, b)
}

#[derive(Copy, Clone)]
pub struct SerializedSignature {
    rbar: [u8; 32],
    sbar: [u8; 32],
}

// #[derive(Copy, Clone)]
pub struct Signature<E: JubjubEngine> {
    pub r: Point<E, Unknown>,
    pub s: E::Fs,
}

pub struct PrivateKey<E: JubjubEngine>(pub E::Fs);

#[derive(Clone)]
pub struct PublicKey<E: JubjubEngine>(pub Point<E, Unknown>);

impl SerializedSignature {
    pub fn read<R: Read>(mut reader: R) -> io::Result<Self> {
        let mut rbar = [0u8; 32];
        let mut sbar = [0u8; 32];
        reader.read_exact(&mut rbar)?;
        reader.read_exact(&mut sbar)?;
        Ok(SerializedSignature { rbar, sbar })
    }

    pub fn write<W: Write>(&self, mut writer: W) -> io::Result<()> {
        writer.write_all(&self.rbar)?;
        writer.write_all(&self.sbar)
    }
}

impl<E: JubjubEngine> PrivateKey<E> {
    pub fn randomize(&self, alpha: E::Fs) -> Self {
        let mut tmp = self.0;
        tmp.add_assign(&alpha);
        PrivateKey(tmp)
    }

    pub fn read<R: Read>(reader: R) -> io::Result<Self> {
        let pk = read_scalar::<E, R>(reader)?;
        Ok(PrivateKey(pk))
    }

    pub fn write<W: Write>(&self, writer: W) -> io::Result<()> {
        write_scalar::<E, W>(&self.0, writer)
    }

    pub fn sign<R: Rng>(
        &self,
        msg: &[u8],
        rng: &mut R,
        p_g: FixedGenerators,
        params: &E::Params,
    ) -> Signature<E> {
        // T = (l_H + 128) bits of randomness
        // For H*, l_H = 128 bits
        let mut t = [0u8; 32];
        rng.fill_bytes(&mut t[..]);

        // Generate randomness using hash function based on some entropy and the message
        // r = H*(T || M)
        let r = h_star_s::<E>(&t[..], msg);

        let pk = PublicKey::from_private(&self, p_g, params);
        let order_check = pk.0.mul(E::Fs::char(), params);
        assert!(order_check.eq(&Point::zero()));

        // R = r . P_G
        let r_g = params.generator(p_g).mul(r, params);

        let (r_g_x, r_g_y) = r_g.into_xy();
        let mut r_g_x_bytes = [0u8; 32];
        r_g_x.into_repr().write_le(& mut r_g_x_bytes[..]).expect("has serialized r_g_x");

        let mut r_g_y_bytes = [0u8; 32];
        r_g_y.into_repr().write_le(& mut r_g_y_bytes[..]).expect("has serialized r_g_y");

        let (pk_x, pk_y) = pk.0.into_xy();
        let mut pk_x_bytes = [0u8; 32];
        pk_x.into_repr().write_le(& mut pk_x_bytes[..]).expect("has serialized pk_x");

        let mut pk_y_bytes = [0u8; 32];
        pk_y.into_repr().write_le(& mut pk_y_bytes[..]).expect("has serialized pk_y");

        let concatenated: Vec<u8> = r_g_x_bytes.iter().chain(r_g_y_bytes.iter()).chain(pk_x_bytes.iter()).chain(pk_y_bytes.iter()).cloned().collect();

        print!("{}\n", concatenated.len() * 8);

        for b in concatenated.clone().into_iter() {
            for i in (0..8).into_iter() {
                if (b & (1 << i) != 0) {
                    print!("{}", 1);
                } else {
                    print!("{}", 0)
                } 
            }
        }
        print!("------------");

        // S = r + H*(Rbar || Pk || M) . sk
        let mut s = h_star_s::<E>(&concatenated[..], msg);
        s.mul_assign(&self.0);
        s.add_assign(&r);
    
        let as_unknown = Point::from(r_g);
        Signature { r: as_unknown, s: s }
    }
}

impl<E: JubjubEngine> PublicKey<E> {
    pub fn from_private(privkey: &PrivateKey<E>, p_g: FixedGenerators, params: &E::Params) -> Self {
        let res = params.generator(p_g).mul(privkey.0, params).into();
        PublicKey(res)
    }

    pub fn randomize(&self, alpha: E::Fs, p_g: FixedGenerators, params: &E::Params) -> Self {
        let res: Point<E, Unknown> = params.generator(p_g).mul(alpha, params).into();
        let res = res.add(&self.0, params);
        PublicKey(res)
    }

    pub fn read<R: Read>(reader: R, params: &E::Params) -> io::Result<Self> {
        let p = Point::read(reader, params)?;
        Ok(PublicKey(p))
    }

    pub fn write<W: Write>(&self, writer: W) -> io::Result<()> {
        self.0.write(writer)
    }

    pub fn verify(
        &self,
        msg: &[u8],
        sig: &Signature<E>,
        p_g: FixedGenerators,
        params: &E::Params,
    ) -> bool {
        // c = H*(Rbar || Pk || M)
        let (r_g_x, r_g_y) = sig.r.into_xy();
        let mut r_g_x_bytes = [0u8; 32];
        r_g_x.into_repr().write_le(& mut r_g_x_bytes[..]).expect("has serialized r_g_x");

        let mut r_g_y_bytes = [0u8; 32];
        r_g_y.into_repr().write_le(& mut r_g_y_bytes[..]).expect("has serialized r_g_y");

        let (pk_x, pk_y) = self.0.into_xy();
        let mut pk_x_bytes = [0u8; 32];
        pk_x.into_repr().write_le(& mut pk_x_bytes[..]).expect("has serialized pk_x");

        let mut pk_y_bytes = [0u8; 32];
        pk_y.into_repr().write_le(& mut pk_y_bytes[..]).expect("has serialized pk_y");

        let concatenated: Vec<u8> = r_g_x_bytes.iter().chain(r_g_y_bytes.iter()).chain(pk_x_bytes.iter()).chain(pk_y_bytes.iter()).cloned().collect();

        let c = h_star_s::<E>(&concatenated[..], msg);

        // this one is for a simple sanity check. In application purposes the pk will always be in a right group 
        let order_check_pk = self.0.mul(E::Fs::char(), params);
        if !order_check_pk.eq(&Point::zero()) {
            return false;
        }

        // r is input from user, so always check it!
        let order_check_r = sig.r.mul(E::Fs::char(), params);
        if !order_check_r.eq(&Point::zero()) {
            return false;
        }

        // self.0.mul(c, params).add(&sig.r, params).add(
        //     &params.generator(p_g).mul(sig.s, params).negate().into(),
        //     params
        // ).mul_by_cofactor(params).eq(&Point::zero());


        // 0 = h_G(-S . P_G + R + c . vk)
        self.0.mul(c, params).add(&sig.r, params).add(
            &params.generator(p_g).mul(sig.s, params).negate().into(),
            params
        ).eq(&Point::zero())
    }

    pub fn verify_serialized(
        &self,
        msg: &[u8],
        sig: &SerializedSignature,
        p_g: FixedGenerators,
        params: &E::Params,
    ) -> bool {
        // c = H*(Rbar || M)
        let c = h_star_s::<E>(&sig.rbar[..], msg);

        // Signature checks:
        // R != invalid
        let r = match Point::read(&sig.rbar[..], params) {
            Ok(r) => r,
            Err(_) => return false,
        };
        // S < order(G)
        // (E::Fs guarantees its representation is in the field)
        let s = match read_scalar::<E, &[u8]>(&sig.sbar[..]) {
            Ok(s) => s,
            Err(_) => return false,
        };
        // 0 = h_G(-S . P_G + R + c . vk)
        self.0.mul(c, params).add(&r, params).add(
            &params.generator(p_g).mul(s, params).negate().into(),
            params
        ).mul_by_cofactor(params).eq(&Point::zero())
    }
}

#[cfg(test)]
mod baby_tests {
    use pairing::bn256::Bn256;
    use rand::thread_rng;

    use alt_babyjubjub::{AltJubjubBn256, fs::Fs, edwards, FixedGenerators};

    use super::*;

    #[test]
    fn cofactor_check() {
        let rng = &mut thread_rng();
        let params = &AltJubjubBn256::new();
        let zero = edwards::Point::zero();
        let p_g = FixedGenerators::SpendingKeyGenerator;

        // Get a point of order 8
        let p8 = loop {
            let r = edwards::Point::<Bn256, _>::rand(rng, params).mul(Fs::char(), params);

            let r2 = r.double(params);
            let r4 = r2.double(params);
            let r8 = r4.double(params);

            if r2 != zero && r4 != zero && r8 == zero {
                break r;
            }
        };

        let sk = PrivateKey::<Bn256>(rng.gen());
        let vk = PublicKey::from_private(&sk, p_g, params);

        let msg = b"Foo bar";
        let sig = sk.sign(msg, rng, p_g, params);
        assert!(vk.verify(msg, &sig, p_g, params));

        // in contrast to redjubjub, in this implementation out-of-group R is NOT allowed!
        let vktorsion = PublicKey(vk.0.add(&p8, params));
        assert!(!vktorsion.verify(msg, &sig, p_g, params));
    }

    // #[test]
    // fn round_trip_serialization() {
    //     let rng = &mut thread_rng();
    //     let p_g = FixedGenerators::SpendingKeyGenerator;
    //     let params = &AltJubjubBn256::new();

    //     for _ in 0..1000 {
    //         let sk = PrivateKey::<Bn256>(rng.gen());
    //         let vk = PublicKey::from_private(&sk, p_g, params);
    //         let msg = b"Foo bar";
    //         let sig = sk.sign(msg, rng, p_g, params);

    //         let mut sk_bytes = [0u8; 32];
    //         let mut vk_bytes = [0u8; 32];
    //         let mut sig_bytes = [0u8; 64];
    //         sk.write(&mut sk_bytes[..]).unwrap();
    //         vk.write(&mut vk_bytes[..]).unwrap();
    //         sig.write(&mut sig_bytes[..]).unwrap();

    //         let sk_2 = PrivateKey::<Bn256>::read(&sk_bytes[..]).unwrap();
    //         let vk_2 = PublicKey::from_private(&sk_2, p_g, params);
    //         let mut vk_2_bytes = [0u8; 32];
    //         vk_2.write(&mut vk_2_bytes[..]).unwrap();
    //         assert!(vk_bytes == vk_2_bytes);

    //         let vk_2 = PublicKey::<Bn256>::read(&vk_bytes[..], params).unwrap();
    //         let sig_2 = Signature::read(&sig_bytes[..]).unwrap();
    //         assert!(vk.verify(msg, &sig_2, p_g, params));
    //         assert!(vk_2.verify(msg, &sig, p_g, params));
    //         assert!(vk_2.verify(msg, &sig_2, p_g, params));
    //     }
    // }

    #[test]
    fn random_signatures() {
        let rng = &mut thread_rng();
        let p_g = FixedGenerators::SpendingKeyGenerator;
        let params = &AltJubjubBn256::new();

        for _ in 0..1000 {
            let sk = PrivateKey::<Bn256>(rng.gen());
            let vk = PublicKey::from_private(&sk, p_g, params);

            let msg1 = b"Foo bar";
            let msg2 = b"Spam eggs";

            let sig1 = sk.sign(msg1, rng, p_g, params);
            let sig2 = sk.sign(msg2, rng, p_g, params);

            assert!(vk.verify(msg1, &sig1, p_g, params));
            assert!(vk.verify(msg2, &sig2, p_g, params));
            assert!(!vk.verify(msg1, &sig2, p_g, params));
            assert!(!vk.verify(msg2, &sig1, p_g, params));

            let alpha = rng.gen();
            let rsk = sk.randomize(alpha);
            let rvk = vk.randomize(alpha, p_g, params);

            let sig1 = rsk.sign(msg1, rng, p_g, params);
            let sig2 = rsk.sign(msg2, rng, p_g, params);

            assert!(rvk.verify(msg1, &sig1, p_g, params));
            assert!(rvk.verify(msg2, &sig2, p_g, params));
            assert!(!rvk.verify(msg1, &sig2, p_g, params));
            assert!(!rvk.verify(msg2, &sig1, p_g, params));
        }
    }
}