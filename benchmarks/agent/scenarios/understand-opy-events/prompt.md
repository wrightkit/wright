`mode.opy` is an Overwatch Workshop game mode written in OverPy. Do not modify it.

Answer these questions about it by writing `answer.json` in this directory, with exactly these keys:

- `deathRules`: the names of the rules whose event is `playerDied`, sorted alphabetically.
- `subroutineCallers`: an object that maps each subroutine name to the sorted list of rule names that call it.
- `neverUsed`: the names of the declared variables that no rule or subroutine reads or writes, sorted alphabetically.
- `multiWriterGlobals`: the names of the global variables written by more than one rule, counting writes made through subroutines that a rule calls, sorted alphabetically.
