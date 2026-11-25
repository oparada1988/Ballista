import os
import json
import locale
from loguru import logger


class LegacyLocaleManager:
    def __init__(self, locales_path: str = ""):
        self.locales_path: str = locales_path or ""
        self.locales_json: dict = {}
        self.fallback_json: dict = {}
        self.locales: str = None
        self.FALLBACK_LOCALE: str = "en_US"
        self.set_fallback_language(self.FALLBACK_LOCALE)

    def load_fallback_language(self):
        if not self.locales_path:
            return
        path = os.path.join(self.locales_path, f"{self.FALLBACK_LOCALE}.json")
        if not os.path.exists(path):
            return
        try:
            with open(path, "r", encoding="utf-8") as f:
                self.fallback_json = json.load(f)
        except Exception as e:
            logger.warning(f"Error loading fallback locale {path}: {e}")

    def set_to_os_default(self):
        try:
            os_locale = locale.getlocale()[0]
        except Exception:
            os_locale = None
        self.set_language(self.FALLBACK_LOCALE if not os_locale else os_locale)

    def set_language(self, language: str) -> str:
        language = self.get_best_match(language or self.FALLBACK_LOCALE)
        self.locales = language
        if not self.locales_path:
            return language
        path = os.path.join(self.locales_path, f"{self.locales}.json")
        if not os.path.isfile(path):
            return language
        try:
            with open(path, "r", encoding="utf-8") as f:
                self.locales_json = json.load(f)
        except Exception as e:
            logger.warning(f"Error loading locale {path}: {e}")
        return language

    def set_fallback_language(self, language: str) -> None:
        self.FALLBACK_LOCALE = language
        self.load_fallback_language()

    def get(self, key: str, fallback: str = None) -> str:
        if fallback is None:
            fallback = key
        result = self.locales_json.get(key, None)
        if result is None:
            result = self.fallback_json.get(key, fallback)
        return result

    def get_availbale_locales(self) -> list:
        locales_list: list[str] = []
        if not self.locales_path or not os.path.isdir(self.locales_path):
            return locales_list
        for fn in os.listdir(self.locales_path):
            if fn.endswith(".json"):
                locales_list.append(os.path.splitext(fn)[0])
        return locales_list

    def get_available_locales(self) -> list:
        return self.get_availbale_locales()

    def get_best_match(self, preferred_language: str) -> str:
        available = self.get_availbale_locales()
        if preferred_language in available:
            return preferred_language
        primary = preferred_language.split("_")[0]
        for lang in available:
            if lang.startswith(primary):
                return lang
        return self.FALLBACK_LOCALE
