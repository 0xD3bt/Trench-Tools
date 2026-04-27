use std::collections::BTreeSet;

use serde_json::json;
use shared_execution_routing::alt_manifest::{AltManifestEntry, shared_alt_manifest_entries};
use solana_sdk::{
    instruction::Instruction,
    message::{AddressLookupTableAccount, v0},
};

const PACKET_LIMIT_BYTES: usize = 1232;

fn diagnostics_enabled() -> bool {
    match std::env::var("ALT_COVERAGE_DIAGNOSTICS") {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        Err(_) => false,
    }
}

pub(crate) fn emit_alt_coverage_diagnostics(
    product: &str,
    label: &str,
    instructions: &[Instruction],
    lookup_tables: &[AddressLookupTableAccount],
    message: &v0::Message,
    serialized_len: Option<usize>,
    extra_manifest_entries: &[AltManifestEntry],
) {
    if !diagnostics_enabled() {
        return;
    }

    let mut manifest_entries = shared_alt_manifest_entries();
    manifest_entries.extend(extra_manifest_entries.iter().cloned());
    let manifest_addresses = manifest_entries
        .iter()
        .map(|entry| entry.address.as_str())
        .collect::<BTreeSet<_>>();

    let emitted_accounts = instructions
        .iter()
        .flat_map(|instruction| {
            std::iter::once(instruction.program_id.to_string()).chain(
                instruction
                    .accounts
                    .iter()
                    .map(|account| account.pubkey.to_string()),
            )
        })
        .collect::<BTreeSet<_>>();

    let signer_count = usize::from(message.header.num_required_signatures);
    let static_non_signers = message
        .account_keys
        .iter()
        .skip(signer_count)
        .map(|key| key.to_string())
        .collect::<BTreeSet<_>>();

    let mut loaded_accounts = BTreeSet::new();
    for lookup in &message.address_table_lookups {
        if let Some(table) = lookup_tables
            .iter()
            .find(|table| table.key == lookup.account_key)
        {
            for index in lookup
                .writable_indexes
                .iter()
                .chain(lookup.readonly_indexes.iter())
            {
                if let Some(address) = table.addresses.get(usize::from(*index)) {
                    loaded_accounts.insert(address.to_string());
                }
            }
        }
    }

    let emitted_manifest_static = static_non_signers
        .intersection(&emitted_accounts)
        .cloned()
        .filter(|account| manifest_addresses.contains(account.as_str()))
        .collect::<BTreeSet<_>>();
    let missing_manifest_static = emitted_manifest_static
        .difference(&loaded_accounts)
        .cloned()
        .collect::<Vec<_>>();

    let estimated_extra_bytes = missing_manifest_static.len() * 31;
    let lookup_tables_used = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();

    eprintln!(
        "[{product}][alt-coverage] {}",
        json!({
            "label": label,
            "lookupTablesUsed": lookup_tables_used,
            "staticNonSignerCount": static_non_signers.len(),
            "loadedAccountCount": loaded_accounts.len(),
            "emittedManifestStatic": emitted_manifest_static.into_iter().collect::<Vec<_>>(),
            "missingManifestStatic": missing_manifest_static,
            "estimatedExtraBytesWithoutAlt": estimated_extra_bytes,
            "serializedBytes": serialized_len,
            "packetHeadroom": serialized_len.map(|len| PACKET_LIMIT_BYTES as isize - len as isize),
        })
    );
}
