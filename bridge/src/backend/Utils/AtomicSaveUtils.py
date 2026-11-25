import json
import os


def atomic_save_json(path: str, data: dict, indent: int = 4):
    parent = os.path.dirname(os.path.abspath(path))
    if parent:
        os.makedirs(parent, exist_ok=True)
    temp = f"{path}.tmp"
    with open(temp, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=indent)
    os.replace(temp, path)
