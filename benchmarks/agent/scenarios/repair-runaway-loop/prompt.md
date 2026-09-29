Players report that as soon as a match starts using `mode.ws` (an Overwatch Workshop game mode), the server freezes and the countdown never finishes.

Find the root cause, fix it with the smallest change that keeps the intended behavior (a counter that counts up once per second while the match runs), and record the name of the offending rule as `{"rule": "<name>"}` in `answer.json`. Make sure the project has no remaining problems.
