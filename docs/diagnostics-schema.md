# Check diagnostics JSON schema

The command emits the published wright-result/v1 envelope. It adds the top-level schema_version value "1" and the provider status field as additive fields. Additive fields remain in wright-result/v1; removing, renaming, or changing the type or meaning of an existing field requires a new result contract version.

The committed schema is schemas/wright-check-v1.schema.json.
