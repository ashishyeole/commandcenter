"""One scheduled collector pass. Run as a managed job, never a permanent worker."""
import json
from server import scheduler_tick
if __name__ == '__main__':
    result = scheduler_tick()
    # Aggregate operational counts only; no tenant IDs, URLs, tokens or source bodies.
    print(json.dumps({'processed':len(result['processed']),'failed':sum(not row['ok'] for row in result['processed'])}))
