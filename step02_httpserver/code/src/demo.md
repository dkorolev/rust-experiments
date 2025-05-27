# Markdown

Hello, I am an example **Markdown** (`*.md`) file!

## Table Example

| Name        | Language            | Purpose                     |
|-----------: | ------------------- | --------------------------- |
| Rust        | Systems Programming | Fast, memory-safe applications |
| Python      | Scripting           | Rapid development, data science |
| JavaScript  | Web                 | Frontend/backend web development |
| Go          | Systems             | Cloud-native services       |

## Mermaid Diagram

```mermaid
flowchart TD
    A[Start] --> B{Is it working?}
    B -->|Yes| C[Great!]
    B -->|No| D[Debug]
    D --> B
    C --> E[Continue]
```

## Sequence Diagram

```mermaid
sequenceDiagram
    participant Browser
    participant Server
    Browser->>Server: GET /markdown
    Server->>Browser: HTML Response
    Browser->>Browser: Render Markdown
    Browser->>Browser: Execute Mermaid JS
    Browser->>Browser: Display Diagrams
```

## Class Diagram

```mermaid
classDiagram
    class Animal {
        +name: string
        +eat(): void
        +sleep(): void
    }
    class Dog {
        +bark(): void
    }
    class Cat {
        +meow(): void
    }
    Animal <|-- Dog
    Animal <|-- Cat
```
