import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "adapters"))

import devin
import pi
import codex
import agy


class PiAdapterTest(unittest.TestCase):
    def test_usage_row_counts_cache_in_context(self):
        message = {"usage": {"input": 586, "output": 5, "cacheRead": 400, "cacheWrite": 100, "reasoning": 3}}
        row = pi.usage_row(message, 272_000, 1.0)
        self.assertEqual((row["input"], row["cache_read"], row["cache_write"], row["context"], row["context_limit"]), (586, 400, 100, 1086, 272_000))
        self.assertEqual((row["output"], row["reasoning"]), (2, 3))
        self.assertEqual(sum(row[key] or 0 for key in ("input", "output", "cache_read", "cache_write", "reasoning")), 1091)

    def test_loaded_skills_and_final_text(self):
        system = {"sections": {"skills": "<available_skills><skill><name>wright</name></skill><skill><name>other</name></skill></available_skills>"}}
        self.assertEqual(pi.skill_names(system), ["wright", "other"])
        self.assertEqual(pi.skill_names({"sections": {}}), [])
        self.assertEqual(pi.message_text({"content": [{"type": "text", "text": "a"}, {"type": "tool_use"}, {"type": "text", "text": "b"}]}), "ab")


class DevinAdapterTest(unittest.TestCase):
    export = {"steps": [
        {"source": "system", "message": '<rules type="always-on">\n<rule name="AGENTS" path="/x/AGENTS.md">'},
        {"source": "system", "message": (
            "<available_skills>\n"
            "- **wright**: A guide. (source: /w/.agents/skills/wright/SKILL.md)\n"
            "- **devin-cli**: Docs. (source: /h/share/devin/docs)\n"
            "- **upload-secrets**: Secrets. (source: builtin:upload-secrets)\n"
            "- **context7-mcp**: Docs. (source: /h/cli/plugins/cache/x/skills/context7-mcp/SKILL.md)\n")},
        {"source": "user", "message": "task"},
        {"source": "agent", "timestamp": "2026-09-30T16:18:40+00:00", "metrics": {"prompt_tokens": 1000, "completion_tokens": 20, "cached_tokens": 600}},
        {"source": "agent", "timestamp": "2026-09-30T16:18:41+00:00", "message": "no metrics"},
    ]}

    def test_usage_rows_split_cached_prompt_tokens(self):
        rows, _, _ = devin.parse_export(self.export)
        self.assertEqual(len(rows), 1)
        self.assertEqual((rows[0]["input"], rows[0]["cache_read"], rows[0]["output"], rows[0]["context"]), (400, 600, 20, 1000))

    def test_loaded_context_excludes_builtins_and_lists_plugins_apart(self):
        _, loaded, plugins = devin.parse_export(self.export)
        self.assertEqual(loaded, ["AGENTS", "wright"])
        self.assertEqual(plugins, ["context7-mcp"])

    def test_config_reads_no_other_tools_and_denies_web_unless_asked(self):
        base = {"read_config_from": {"claude": True}, "permissions": {"allow": ["Exec(*)"]}, "agent": {"model": "old"}}
        closed = devin.isolated_config(base, "swe-2-max", web=False)
        opened = devin.isolated_config(base, "swe-2-max", web=True)
        self.assertFalse(any(closed["read_config_from"].values()))
        self.assertEqual(closed["agent"]["model"], "swe-2-max")
        self.assertIn("web_search", closed["permissions"]["deny"])
        self.assertNotIn("web_search", opened["permissions"]["deny"])
        self.assertIn("mcp_call_tool", opened["permissions"]["deny"])
        self.assertNotIn("allow", closed["permissions"])


class NativeAdapterUsageTest(unittest.TestCase):
    def test_codex_inclusive_counts_are_split_without_counting_reasoning_twice(self):
        row = codex.usage_row({"input_tokens": 100, "cached_input_tokens": 60, "output_tokens": 20, "reasoning_output_tokens": 12}, 1.0, 272000)
        self.assertEqual((row["input"], row["cache_read"], row["output"], row["reasoning"], row["context"]), (40, 60, 8, 12, 100))
        self.assertEqual(sum(row[key] or 0 for key in ("input", "cache_read", "cache_write", "output", "reasoning")), 120)

    def test_agy_cache_is_exclusive_and_thinking_is_included_in_output(self):
        row = agy.usage_row({"input_tokens": 278, "cache_read_tokens": 30214, "output_tokens": 4, "thinking_tokens": 3}, 1.0)
        self.assertEqual((row["input"], row["cache_read"], row["output"], row["reasoning"], row["context"]), (278, 30214, 1, 3, 30492))
        self.assertIsNone(row["context_limit"])

    def test_codex_builtin_skills_are_separate_from_observed_project_skills(self):
        text = ('- `r0` = `/isolated/.codex/skills/.system`\n- `r1` = `/workspace/.agents/skills`\n'
                '- openai-docs: Builtin. (file: r0/openai-docs/SKILL.md)\n'
                '- wright: Project. (file: r1/wright/SKILL.md)\n')
        self.assertEqual(codex.loaded_skills(text), (["wright"], ["openai-docs"]))


if __name__ == "__main__":
    unittest.main()
