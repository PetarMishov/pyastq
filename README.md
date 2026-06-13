TODOs:

    1. Search by patterns:
        - past find "call:eval" -> returns calls to a function called "eval" (maybe color code / return the functions in groups since multiple functions might be called the same)
        - past find "import:requests"
        - past find "class:User"
        EXTRA: make this work with regex 
        - past find "class:(contains User)" (finds all classes that contain "User" in the name)(also with ends_with starts_with)
    
    2. (UNSURE) Structural search:
        - past find "function_definitions"

    3. Function/Class inventory:
        - past functions -> return all functions
        - past classes -> return all classes

    4. Import graph:
        - past imports main.py -> 
            imports:
                train.py
                    torch
                os
                pandas
    
    5. Who imports:
        - past who-imports train.py -> main.py
    
    6. (UNSURE) Summary:
        - past summary -> Files 100, Classes 20, Top imports: opencv, ...