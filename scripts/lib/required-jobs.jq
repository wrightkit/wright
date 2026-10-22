{
  "agent-guide": ($p.agent_guide == "true"),
  "cli-behavior": ($p.cli == "true"),
  "dist-cli-build": ($p.dist == "true"),
  "dist-validation": ($p.dist == "true"),
  "lpp-integration": ($p.lpp == "true"),
  "metrics-drift": ($p.opy == "true"),
  "metrics-shard": ($p.opy == "true"),
  "opy-integration": ($p.opy == "true"),
  "rust-quality": ($p.rust_core == "true"),
  "ui": ($p.ui == "true"),
  "workshop-integration": ($p.rust_core == "true"),
  "wright-cli-build": ($p.opy == "true" or $p.cli == "true")
}
