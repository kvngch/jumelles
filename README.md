# Jumelles

Outil Windows qui retrouve les photos en double ou similaires. Il est en cours d'écriture, la première version sera la 0.1.0 ([jalon v0.1.0](https://github.com/kvngch/jumelles/milestone/1)).

## Compiler

Avec Rust stable, sous Windows 10 ou 11 :

```
cargo build --release -p jumelles
```

L'exe est `target\release\jumelles.exe`. Le moteur, dans `core`, se teste aussi sous Linux :

```
cargo test -p jumelles-core
```

## Licence

GPL-3.0 ou ultérieure, texte complet dans [LICENSE](LICENSE).
