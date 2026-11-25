import os
import subprocess
import sys
from venv import create


def create_venv(path: str = ".venv", path_to_requirements_txt: str = None) -> None:
    create(path, with_pip=True)
    if path_to_requirements_txt is None or not os.path.isfile(path_to_requirements_txt):
        return
    pip_bin = os.path.join(path, "bin", "pip")
    if os.path.isfile(pip_bin):
        subprocess.run([pip_bin, "install", "-r", path_to_requirements_txt], check=False)
    else:
        subprocess.run(
            f". {os.path.join(path, 'bin', 'activate')} && pip install -r {path_to_requirements_txt}",
            shell=True,
            check=False,
        )
