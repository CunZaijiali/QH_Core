use qh_macros::define_id;

define_id!(pub NumericId, u64);
define_id!(pub TextId, String);

#[test]
fn generates_numeric_id_behavior() {
    let id = NumericId::new(42_u64);

    assert_eq!(id.value(), 42);
    assert_eq!(id.to_string(), "42");
    assert_eq!(NumericId::from(42_u64), id);
    assert_eq!(u64::from(id), 42);
}

#[test]
fn generates_string_id_behavior_and_serde() {
    let id = TextId::from("session-1");
    let encoded = serde_json::to_string(&id).unwrap();
    let decoded: TextId = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, id);
    assert_eq!(decoded.to_string(), "session-1");
    assert_eq!(String::from(decoded), "session-1");
}
