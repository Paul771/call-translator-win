"""Entry point — run the Flask web UI."""

import logging
import sys
import os

from web import app

if __name__ == "__main__":
    logging.basicConfig(level=logging.DEBUG, format="%(asctime)s [%(levelname)s] %(name)s: %(message)s")
    logging.getLogger("werkzeug").setLevel(logging.WARNING)

    # Debug: log Python path and pycaw availability
    logger = logging.getLogger("translator")
    logger.info(f"Python: {sys.executable}")
    logger.info(f"PID: {os.getpid()}")

    try:
        from web.helpers import list_audio_devices
        result = list_audio_devices()
        logger.info(f"Devices at startup: input={len(result.get('input', []))}, output={len(result.get('output', []))}")
        for d in result.get('input', []):
            logger.info(f"  IN:  {d}")
        for d in result.get('output', []):
            logger.info(f"  OUT: {d}")
    except Exception as e:
        logger.error(f"Device enumeration failed at startup: {e}", exc_info=True)

    app.run(host="127.0.0.1", port=5050, debug=False, threaded=True)
