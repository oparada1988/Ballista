import os
import base64
import io
import webbrowser


def is_image(path: str) -> bool:
    if not path or not isinstance(path, str):
        return False
    ext = os.path.splitext(path)[1].lower().lstrip(".")
    return ext in ("png", "jpg", "jpeg", "webp", "bmp", "ico")


def is_svg(path: str) -> bool:
    if not path or not isinstance(path, str):
        return False
    return path.lower().endswith(".svg")


def is_video(path: str) -> bool:
    if not path or not isinstance(path, str):
        return False
    ext = os.path.splitext(path)[1].lower().lstrip(".")
    return ext in ("mp4", "mov", "mkv", "webm", "gif")


def get_last_dir(path: str) -> str:
    return os.path.basename(os.path.normpath(path))


def open_web(url: str) -> None:
    try:
        webbrowser.open(url)
    except Exception:
        pass


def find_fallback_font() -> str:
    return "Cantarell"


def recursive_hasattr(obj, attr: str) -> bool:
    for part in attr.split("."):
        if not hasattr(obj, part):
            return False
        obj = getattr(obj, part)
    return True


def image2base64(image) -> str:
    if image is None:
        return ""
    buf = io.BytesIO()
    image.save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode("utf-8")
