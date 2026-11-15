"""Loads the driver test kit's Python scripts the way a tester's PC does.

A tester installs whatever Python python.org offers that day, then the kit's
requirements.txt and soundcard, and runs the kit's scripts from the kit folder
(docs/testing/test-signed-pc.md). CI used one Python, so a pin without a wheel
for another Python, or a script that needs a newer Python than the guide says,
reached the tester first. This check runs on every Python the guide allows:

* it copies the Python files driver-test-kit.ps1 puts in the kit to an empty
  folder, so an import the kit does not carry fails here,
* imports each of them there with warnings as errors, and
* imports soundcard the way the scripts do (numpy_compat first).

    python kit_runtime_check.py

Exit status 0 when everything loads, 1 otherwise.
"""

import importlib
import re
import shutil
import sys
import tempfile
import warnings
from pathlib import Path

HERE = Path(__file__).resolve().parent
KIT_SCRIPT = HERE / "driver-test-kit.ps1"


def kit_python_files(text):
    """The .py names the kit script copies from tools/ci, in order, once each."""
    names = []
    for name in re.findall(r"'([A-Za-z0-9_]+\.py)'", text):
        if name not in names:
            names.append(name)
    return names


def main():
    names = kit_python_files(KIT_SCRIPT.read_text(encoding="utf-8-sig"))
    if not names:
        print(f"no Python files found in {KIT_SCRIPT.name}", file=sys.stderr)
        return 1
    problems = []
    with tempfile.TemporaryDirectory() as folder:
        for name in names:
            shutil.copy(HERE / name, folder)
        # The kit folder only: tools/ci must not supply a module the kit lacks.
        sys.path[:] = [folder] + [entry for entry in sys.path
                                  if entry and Path(entry).resolve() != HERE]
        warnings.simplefilter("error")
        for name in names:
            module = name[:-3]
            try:
                importlib.import_module(module)
                print(f"ok   {name}")
            except BaseException as error:  # SystemExit from a script counts too
                problems.append(f"{name}: {type(error).__name__}: {error}")
                print(f"FAIL {name}: {type(error).__name__}: {error}")
        try:
            import numpy as np
            from numpy_compat import binary_fromstring
            np.fromstring = binary_fromstring
            warnings.simplefilter("default")  # soundcard's own warnings are not ours
            import soundcard
            print(f"ok   {soundcard.__name__} "
                  f"with numpy {np.__version__} on Python {sys.version.split()[0]}")
        except BaseException as error:
            problems.append(f"soundcard: {type(error).__name__}: {error}")
            print(f"FAIL soundcard: {type(error).__name__}: {error}")
        # Later imports must not look in the folder once it is deleted.
        sys.path[:] = [entry for entry in sys.path if entry != folder]
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
