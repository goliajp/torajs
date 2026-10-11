use super::*;
use crate::ct::ct_eq;
use crate::sha2::{Sha256, Sha384, Sha512};
use crate::testutil::{hex, unhex};

struct Case {
    n: u32,
    key: &'static str,
    data: &'static str,
    sha256: &'static str,
    sha384: &'static str,
    sha512: &'static str,
}

/// RFC 4231 §4.2–§4.8, test cases 1–7 for HMAC-SHA-256/384/512. Case 5
/// lists only the leftmost 128 bits of each tag.
const RFC4231: &[Case] = &[
    Case {
        n: 1,
        key: "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b",
        data: "4869205468657265",
        sha256: "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
        sha384: "afd03944d84895626b0825f4ab46907f15f9dadbe4101ec682aa034c7cebc59cfaea9ea9076ede7f4af152e8b2fa9cb6",
        sha512: "87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cdedaa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854",
    },
    Case {
        n: 2,
        key: "4a656665",
        data: "7768617420646f2079612077616e7420666f72206e6f7468696e673f",
        sha256: "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
        sha384: "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649",
        sha512: "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737",
    },
    Case {
        n: 3,
        key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        data: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        sha256: "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
        sha384: "88062608d3e6ad8a0aa2ace014c8a86f0aa635d947ac9febe83ef4e55966144b2a5ab39dc13814b94e3ab6e101a34f27",
        sha512: "fa73b0089d56a284efb0f0756c890be9b1b5dbdd8ee81a3655f83e33b2279d39bf3e848279a722c806b485a47e67c807b946a337bee8942674278859e13292fb",
    },
    Case {
        n: 4,
        key: "0102030405060708090a0b0c0d0e0f10111213141516171819",
        data: "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        sha256: "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b",
        sha384: "3e8a69b7783c25851933ab6290af6ca77a9981480850009cc5577c6e1f573b4e6801dd23c4a7d679ccf8a386c674cffb",
        sha512: "b0ba465637458c6990e5a8c5f61d4af7e576d97ff94b872de76f8050361ee3dba91ca5c11aa25eb4d679275cc5788063a5f19741120c4f2de2adebeb10a298dd",
    },
    Case {
        n: 5,
        key: "0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c",
        data: "546573742057697468205472756e636174696f6e",
        sha256: "a3b6167473100ee06e0c796c2955552b",
        sha384: "3abf34c3503b2a23a46efc619baef897",
        sha512: "415fad6271580a531d4179bc891d87a6",
    },
    Case {
        n: 6,
        key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        data: "54657374205573696e67204c6172676572205468616e20426c6f636b2d53697a65204b6579202d2048617368204b6579204669727374",
        sha256: "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
        sha384: "4ece084485813e9088d2c63a041bc5b44f9ef1012a2b588f3cd11f05033ac4c60c2ef6ab4030fe8296248df163f44952",
        sha512: "80b24263c7c1a3ebb71493c1dd7be8b49b46d1f41b4aeec1121b013783f8f3526b56d037e05f2598bd0fd2215d6a1e5295e64f73f63f0aec8b915a985d786598",
    },
    Case {
        n: 7,
        key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        data: "5468697320697320612074657374207573696e672061206c6172676572207468616e20626c6f636b2d73697a65206b657920616e642061206c6172676572207468616e20626c6f636b2d73697a6520646174612e20546865206b6579206e6565647320746f20626520686173686564206265666f7265206265696e6720757365642062792074686520484d414320616c676f726974686d2e",
        sha256: "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2",
        sha384: "6617178e941f020d351e2f254e8fd32c602420feb0b8fb9adccebb82461e99c5a678cc31e799176d3860e6110c46523e",
        sha512: "e37b6a775dc87dbaa4dfa9f96e5e3ffddebd71f8867289865df5a32d20cdc944b6022cac3c4982b10d5eeb55c3e4de15134676fb6de0446065c97440fa8c6a58",
    },
];

fn check<D: Digest>(c: &Case, want: &str) {
    let key = unhex(c.key);
    let data = unhex(c.data);
    let tag = hex(Hmac::<D>::mac(&key, &data).as_ref());
    assert!(tag.starts_with(want), "case {}: {tag} vs {want}", c.n);

    let want = unhex(want);
    let mac = || Hmac::<D>::new(&key).chain(&data);
    assert_eq!(mac().verify(&want).is_ok(), want.len() == D::OUTPUT_LEN);
    let allowed = want.len() >= (D::OUTPUT_LEN / 2).max(10);
    assert_eq!(mac().verify_truncated(&want).is_ok(), allowed);
    let mut bad = want.clone();
    bad[0] ^= 1;
    assert!(mac().verify_truncated(&bad).is_err());
    assert!(mac().verify(&bad).is_err());
}

#[test]
fn rfc4231_sha256() {
    for c in RFC4231 {
        check::<Sha256>(c, c.sha256);
    }
}

#[test]
fn rfc4231_sha384() {
    for c in RFC4231 {
        check::<Sha384>(c, c.sha384);
    }
}

#[test]
fn rfc4231_sha512() {
    for c in RFC4231 {
        check::<Sha512>(c, c.sha512);
    }
}

#[test]
fn streaming_matches_one_shot() {
    let key = [0x42u8; 77];
    let data: std::vec::Vec<u8> = (0..500u32).map(|i| i as u8).collect();
    let want = Hmac::<Sha512>::mac(&key, &data);
    let mut mac = Hmac::<Sha512>::new(&key);
    for piece in data.chunks(31) {
        mac.update(piece);
    }
    assert!(ct_eq(&mac.finish(), &want).reveal());
}

#[test]
fn truncated_tag_length_limits() {
    let tag = Hmac::<Sha256>::mac(b"k", b"m");
    let mac = || Hmac::<Sha256>::new(b"k").chain(b"m");
    assert!(mac().verify_truncated(&tag[..16]).is_ok());
    assert_eq!(mac().verify_truncated(&tag[..15]), Err(MacError));
    assert!(mac().verify_truncated(&tag).is_ok());
    assert_eq!(mac().verify(&tag[..16]), Err(MacError));
    assert_eq!(std::format!("{:?}", mac()), "Hmac");
}
