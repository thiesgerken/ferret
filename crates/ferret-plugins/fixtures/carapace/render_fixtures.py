"""Renders the carapace plugin's fixtures with carapace's own memory mirror renderer.

Rendered by carapace itself, so the parser is tested against what carapace actually writes. Run
it with carapace's locked dependencies, but in a throwaway environment, so carapace's own
`.venv` stays as it is:

    UV_PROJECT_ENVIRONMENT=/tmp/carapace-fixtures-venv \
        uv run --frozen --project <carapace checkout> python render_fixtures.py <this directory>

The records cover what the parser has to handle: facts with and without a subject, dated facts,
several source refs, a statement that contains the meta separators, prose with escaped block
markers, sessions with empty sections, and a week and a month digest.
"""

from __future__ import annotations

import sys
from datetime import UTC, date, datetime
from decimal import Decimal
from pathlib import Path

from carapace.memory.models import (
    CoverageEntry,
    DigestFact,
    DigestLevel,
    DigestRecord,
    DigestTheme,
    ExtractionRecord,
    Fact,
    PeriodDigest,
    Provenance,
    SessionExtraction,
)
from carapace.memory.render import render_mirror


def provenance(task_id: int) -> Provenance:
    return Provenance(
        carapace_version="0.158.7",
        model="anthropic:claude-haiku-4-5",
        prompt_version="a1b2c3d4e5f6",
        input_format_version=1,
        input_hash="f" * 64,
        input_tokens=1200,
        output_tokens=300,
        cost_usd=Decimal("0.0042"),
        duration_ms=2100,
        task_id=task_id,
        created_at=datetime(2026, 9, 8, 7, 12, tzinfo=UTC),
    )


TALOS = ExtractionRecord(
    id=1,
    session_id="s-talos",
    week_key="2026-W36",
    month_key="2026-09",
    is_current=True,
    input_hash="f" * 64,
    provenance=provenance(7),
    created_at=datetime(2026, 9, 8, 7, 12, tzinfo=UTC),
    extraction=SessionExtraction(
        abstract=(
            "The user planned the Talos upgrade of the home cluster.\n\n"
            "# not a heading, just a line the model started with a hash\n"
            "- nor a list item"
        ),
        outcomes=["Decided to upgrade the control plane first.", "Picked Saturday for it."],
        open_loops=["Back up etcd before the upgrade."],
        on_my_mind=["upgrading the home cluster to Talos 1.11"],
        facts=[
            Fact(
                category="surroundings",
                statement="The home cluster runs Talos 1.10.",
                subject="home cluster",
                source_seqs=[4],
                source_kind="observed",
                confidence="high",
                durability="dated",
                valid_until=date(2026, 10, 1),
            ),
            Fact(
                category="user",
                statement="The user prefers upgrading on weekends.",
                source_seqs=[2, 9],
                source_kind="user_said",
                confidence="medium",
                durability="durable",
            ),
            Fact(
                category="user",
                statement="The user writes notes like _(this · or that)_ inline.",
                source_seqs=[11],
                source_kind="user_said",
                confidence="low",
                durability="durable",
            ),
            Fact(
                category="social",
                statement="Anna helps with the cluster now and then.",
                subject="Anna",
                source_seqs=[5, 6, 7],
                source_kind="user_said",
                confidence="high",
                durability="durable",
            ),
        ],
        friction=["The upgrade docs were hard to find."],
        tags=["homelab", "talos"],
    ),
)

TRIP = ExtractionRecord(
    id=2,
    session_id="s-trip",
    week_key="2026-W36",
    month_key="2026-09",
    is_current=True,
    input_hash="e" * 64,
    provenance=provenance(8),
    created_at=datetime(2026, 9, 6, 18, 30, tzinfo=UTC),
    extraction=SessionExtraction(
        abstract="The user planned a weekend trip with Anna.",
        outcomes=[],
        open_loops=[],
        on_my_mind=[],
        facts=[
            Fact(
                category="social",
                statement="Anna is the user's sister.",
                subject="Anna",
                source_seqs=[3],
                source_kind="user_said",
                confidence="high",
                durability="durable",
            ),
        ],
        friction=[],
        tags=[],
    ),
)

WEEK = DigestRecord(
    id=3,
    level=DigestLevel.week,
    period_key="2026-W36",
    is_current=True,
    coverage=[
        CoverageEntry(source_id="s-talos", source_hash="abc"),
        CoverageEntry(source_id="s-trip", source_hash="def"),
    ],
    coverage_hash="c0ffee",
    provenance=provenance(9),
    created_at=datetime(2026, 9, 8, 8, 0, tzinfo=UTC),
    digest=PeriodDigest(
        summary="Mostly homelab work, and a trip with Anna.",
        on_my_mind=[DigestTheme(theme="Talos upgrade", refs=["s-talos"])],
        highlights=["Planned the Talos upgrade."],
        open_loops=["Back up etcd."],
        learned=[
            DigestFact(
                category="social",
                statement="Anna is the user's sister.",
                subject="Anna",
                source_kind="user_said",
                confidence="high",
                durability="durable",
                refs=["s-trip"],
            ),
        ],
    ),
)

MONTH = DigestRecord(
    id=4,
    level=DigestLevel.month,
    period_key="2026-09",
    is_current=True,
    coverage=[CoverageEntry(source_id="2026-W36", source_hash="123")],
    coverage_hash="beef",
    provenance=provenance(11),
    created_at=datetime(2026, 10, 1, 6, 0, tzinfo=UTC),
    digest=PeriodDigest(
        summary="A homelab month.",
        on_my_mind=[DigestTheme(theme="Talos upgrade", refs=["2026-W36"])],
        highlights=["Upgraded the cluster."],
        open_loops=[],
        learned=[],
    ),
)


def main() -> None:
    root = Path(sys.argv[1])
    for path, content in render_mirror([TRIP, TALOS], [WEEK, MONTH]).items():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")
        print(path)


if __name__ == "__main__":
    main()
