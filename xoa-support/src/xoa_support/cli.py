"""Console entry points (installed via pyproject [project.scripts]).

Thin shims around the standalone scripts in bin/ so the tools work both as
`python3 bin/xoa_scrub.py` on an airgapped XOA and as installed
`xoa-scrub` / `xoa-bundle-inspect` commands.
"""

import os
import sys

_BIN = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "bin")
_BIN = os.path.normpath(_BIN)
if os.path.isdir(_BIN) and _BIN not in sys.path:
    sys.path.insert(0, _BIN)


def main(argv=None):
    import xoa_scrub

    return xoa_scrub.main(argv)


def inspect_main(argv=None):
    import xoa_bundle_inspect

    return xoa_bundle_inspect.main(argv)
