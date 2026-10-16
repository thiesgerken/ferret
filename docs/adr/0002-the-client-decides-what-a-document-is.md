# The ingesting client decides what a document is

The server knows only documents and chunks them itself. It has no structure for chat messages,
mail threads or facts: a chat plugin cuts conversations into segments and sends each as a plain
document, mails are one document each, related documents are linked by an optional `thread`. A
message table for chats was considered and rejected, even though ferret holds the only copy of
chat exports: one generic contract keeps the server small and every plugin equal, and the viewer
gets conversation context from thread navigation.
