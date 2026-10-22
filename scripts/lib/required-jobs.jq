def spec:
  {
    "agent-guide":           ["agent_guide"],
    "cli-behavior":          ["cli"],
    "dist-cli-build":        ["dist"],
    "dist-validation":       ["dist"],
    "lpp-integration":       ["lpp"],
    "metrics-drift":         ["opy"],
    "metrics-shard":         ["opy"],
    "opy-integration":       ["opy"],
    "rust-quality":          ["rust_core"],
    "ui":                    ["ui"],
    "workshop-integration":  ["rust_core"],
    "wright-cli-build":      ["opy", "cli"]
  };
spec | with_entries(.value = (.value | map($p[.] == "true") | any))
