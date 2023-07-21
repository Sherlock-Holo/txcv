# txcv

tencent cloud translate console version

## Usage

```shell
Usage: txcv [OPTIONS] [WORDS]...

Arguments:
  [WORDS]...

Options:
  -c, --clear            clear authentication
  -s, --source <SOURCE>  source language, default is auto detect [possible values: chinese, english, japanese]
  -t, --target <TARGET>  target language, default is auto detect [possible values: chinese, english, japanese]
  -h, --help             Print help
```

## Example

```shell
txcv test

test -> 测试
```

## About authentication

first time run txcv will ask your secret id, secret key and api region, txcv will use your system
keyring to store these authentication info

you should generate your own secret id and secret key on the tencentcloud web console

## License

MIT
