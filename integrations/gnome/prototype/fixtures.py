"""Synthetic repositories only; never use user shell profiles in native tests."""

from pathlib import Path
import subprocess


def repositories(directory, labels=("alpha", "beta")):
    result = []
    for label in labels:
        project = Path(directory) / label
        project.mkdir()
        subprocess.run(["git", "init", "--quiet", "--initial-branch", f"gnome-{label}",
                        str(project)], check=True)
        subprocess.run(["git", "-C", str(project), "-c", "user.name=Lyn fixture",
                        "-c", "user.email=fixture@localhost", "-c", "commit.gpgsign=false",
                        "commit", "--quiet", "--allow-empty", "-m", "Native fixture"], check=True)
        branch = subprocess.check_output(["git", "-C", str(project), "branch", "--show-current"],
                                         text=True).strip()
        assert branch == f"gnome-{label}"
        result.append(project)
    return result
