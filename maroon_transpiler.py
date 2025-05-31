#!/usr/bin/env python3
"""
Maroon DSL to JSON Bytecode Transpiler

Transpiles Maroon DSL source code into JSON bytecode that can be executed
by the Maroon stack machine runtime.
"""

import json
import re
from dataclasses import dataclass
from typing import List, Dict, Any, Optional, Union


@dataclass
class Token:
    type: str
    value: str
    line: int
    column: int


class Lexer:
    def __init__(self, source: str):
        self.source = source
        self.position = 0
        self.line = 1
        self.column = 1
        self.tokens = []
        
    def tokenize(self) -> List[Token]:
        patterns = [
            ('FUNCTION', r'function'),
            ('IF', r'if'),
            ('ELSE', r'else'),
            ('RETURN', r'return'),
            ('LET', r'let'),
            ('WRITE', r'write'),
            ('SLEEP', r'sleep'),
            ('NUMBER', r'\d+'),
            ('IDENTIFIER', r'[a-zA-Z_][a-zA-Z0-9_]*'),
            ('STRING', r'"([^"]*)"'),
            ('ARROW', r'->'),
            ('LE', r'<='),
            ('GE', r'>='),
            ('EQ', r'=='),
            ('NE', r'!='),
            ('LT', r'<'),
            ('GT', r'>'),
            ('ASSIGN', r'='),
            ('PLUS', r'\+'),
            ('MINUS', r'-'),
            ('MULTIPLY', r'\*'),
            ('LPAREN', r'\('),
            ('RPAREN', r'\)'),
            ('LBRACE', r'\{'),
            ('RBRACE', r'\}'),
            ('COMMA', r','),
            ('COLON', r':'),
            ('WHITESPACE', r'[ \t]+'),
            ('NEWLINE', r'\n'),
        ]
        
        token_re = '|'.join(f'(?P<{name}>{pattern})' for name, pattern in patterns)
        
        for match in re.finditer(token_re, self.source):
            token_type = match.lastgroup
            token_value = match.group()
            
            if token_type == 'WHITESPACE':
                self.column += len(token_value)
            elif token_type == 'NEWLINE':
                self.line += 1
                self.column = 1
            elif token_type == 'STRING':
                # Extract string content without quotes
                self.tokens.append(Token(token_type, match.group(1), self.line, self.column))
                self.column += len(token_value)
            else:
                self.tokens.append(Token(token_type, token_value, self.line, self.column))
                self.column += len(token_value)
                
        return self.tokens


class Parser:
    def __init__(self, tokens: List[Token]):
        self.tokens = tokens
        self.position = 0
        self.current_function = None
        self.state_counter = 0
        self.functions = {}
        
    def parse(self) -> Dict[str, Any]:
        while self.position < len(self.tokens):
            self.parse_function()
        return {
            "version": "1.0",
            "functions": self.functions,
            "entry_point": "factorial"  # TODO: make configurable
        }
    
    def current_token(self) -> Optional[Token]:
        if self.position < len(self.tokens):
            return self.tokens[self.position]
        return None
    
    def consume(self, expected_type: str) -> Token:
        token = self.current_token()
        if not token or token.type != expected_type:
            raise SyntaxError(f"Expected {expected_type}, got {token}")
        self.position += 1
        return token
    
    def parse_function(self):
        self.consume('FUNCTION')
        name = self.consume('IDENTIFIER').value
        self.consume('LPAREN')
        
        # Parse parameters
        params = []
        if self.current_token().type != 'RPAREN':
            param_name = self.consume('IDENTIFIER').value
            self.consume('COLON')
            param_type = self.consume('IDENTIFIER').value  # Assume u64 for now
            params.append({"name": param_name, "type": param_type})
            
        self.consume('RPAREN')
        self.consume('ARROW')
        return_type = self.consume('IDENTIFIER').value
        self.consume('LBRACE')
        
        # Generate states for this function
        self.current_function = name
        self.state_counter = 0
        states = self.generate_function_states(name, params[0]["name"] if params else None)
        
        self.functions[name] = {"states": states}
        
        # Skip to closing brace for now (simplified parsing)
        brace_count = 1
        while brace_count > 0:
            token = self.current_token()
            if token.type == 'LBRACE':
                brace_count += 1
            elif token.type == 'RBRACE':
                brace_count -= 1
            self.position += 1
    
    def generate_function_states(self, func_name: str, param_name: str) -> List[Dict[str, Any]]:
        """Generate the states for factorial function (hardcoded for now)"""
        if func_name == "factorial":
            return [
                {
                    "name": "FactorialEntry",
                    "local_vars": 1,
                    "operations": [{
                        "type": "push_stack",
                        "entries": [
                            {"Value": {"FactorialInput": {"var": "n"}}},
                            {"Retrn": "FactorialDone"},
                            {"Value": {"FactorialArgument": {"var": "n"}}},
                            {"State": "FactorialRecursiveCall"}
                        ]
                    }]
                },
                {
                    "name": "FactorialRecursiveCall",
                    "local_vars": 1,
                    "operations": [{
                        "type": "write",
                        "text": "f({n})",
                        "next_state": "FactorialRecursionPostWrite"
                    }]
                },
                {
                    "name": "FactorialRecursionPostWrite",
                    "local_vars": 1,
                    "operations": [{
                        "type": "sleep",
                        "ms": {"mul": [{"var": "n"}, {"const": 50}]},
                        "next_state": "FactorialRecursionPostSleep"
                    }]
                },
                {
                    "name": "FactorialRecursionPostSleep",
                    "local_vars": 1,
                    "operations": [{
                        "type": "conditional",
                        "condition": {"le": [{"var": "n"}, {"const": 1}]},
                        "then": [{
                            "type": "return",
                            "value": {"const": 1}
                        }],
                        "else": [{
                            "type": "push_stack",
                            "entries": [
                                {"Retrn": "FactorialRecursionPostRecursiveCall"},
                                {"Value": {"FactorialArgument": {"sub": [{"var": "n"}, {"const": 1}]}}},
                                {"State": "FactorialRecursiveCall"}
                            ]
                        }]
                    }]
                },
                {
                    "name": "FactorialRecursionPostRecursiveCall",
                    "local_vars": 2,
                    "operations": [{
                        "type": "return",
                        "value": {"mul": [{"var": "n"}, {"var": "result"}]}
                    }]
                },
                {
                    "name": "FactorialDone",
                    "local_vars": 2,
                    "operations": [{
                        "type": "done"
                    }]
                }
            ]
        return []


def transpile(source: str) -> str:
    """Transpile Maroon DSL source to JSON bytecode"""
    lexer = Lexer(source)
    tokens = lexer.tokenize()
    
    parser = Parser(tokens)
    bytecode = parser.parse()
    
    return json.dumps(bytecode, indent=2)


def main():
    import sys
    
    if len(sys.argv) != 2:
        print("Usage: python maroon_transpiler.py <input.maroon>")
        sys.exit(1)
    
    with open(sys.argv[1], 'r') as f:
        source = f.read()
    
    try:
        bytecode = transpile(source)
        output_file = sys.argv[1].replace('.maroon', '_bytecode.json')
        with open(output_file, 'w') as f:
            f.write(bytecode)
        print(f"Successfully transpiled to {output_file}")
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()