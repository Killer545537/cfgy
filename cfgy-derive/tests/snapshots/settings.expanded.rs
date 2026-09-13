#[automatically_derived]
impl ::cfgy::FromValue for Settings {
    fn from_value(
        __cfgy_value: &::cfgy::Value,
        __cfgy_path: &mut ::cfgy::PathStack,
    ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
        let ::cfgy::Value::Map(__cfgy_map) = __cfgy_value else {
            return ::core::result::Result::Err(
                ::cfgy::ConfigError::type_mismatch(__cfgy_path, "table", __cfgy_value),
            );
        };
        ::core::result::Result::Ok(Self {
            kind: match __cfgy_map.get("type") {
                ::core::option::Option::Some(__cfgy_field) => {
                    __cfgy_path
                        .with_key(
                            "type",
                            |__cfgy_path| <String as ::cfgy::FromValue>::from_value(
                                __cfgy_field,
                                __cfgy_path,
                            ),
                        )?
                }
                ::core::option::Option::None => {
                    return ::core::result::Result::Err(
                        __cfgy_path
                            .with_key(
                                "type",
                                |__cfgy_path| ::cfgy::ConfigError::missing(__cfgy_path),
                            ),
                    );
                }
            },
            server: match __cfgy_map.get("server") {
                ::core::option::Option::Some(__cfgy_field) => {
                    __cfgy_path
                        .with_key(
                            "server",
                            |__cfgy_path| <Server as ::cfgy::FromValue>::from_value(
                                __cfgy_field,
                                __cfgy_path,
                            ),
                        )?
                }
                ::core::option::Option::None => {
                    return ::core::result::Result::Err(
                        __cfgy_path
                            .with_key(
                                "server",
                                |__cfgy_path| ::cfgy::ConfigError::missing(__cfgy_path),
                            ),
                    );
                }
            },
            replicas: match __cfgy_map.get("replicas") {
                ::core::option::Option::Some(__cfgy_field) => {
                    __cfgy_path
                        .with_key(
                            "replicas",
                            |__cfgy_path| <Vec<
                                Server,
                            > as ::cfgy::FromValue>::from_value(
                                __cfgy_field,
                                __cfgy_path,
                            ),
                        )?
                }
                ::core::option::Option::None => {
                    return ::core::result::Result::Err(
                        __cfgy_path
                            .with_key(
                                "replicas",
                                |__cfgy_path| ::cfgy::ConfigError::missing(__cfgy_path),
                            ),
                    );
                }
            },
            description: match __cfgy_map.get("description") {
                ::core::option::Option::Some(::cfgy::Value::Null) => {
                    ::core::option::Option::None
                }
                ::core::option::Option::Some(__cfgy_field) => {
                    ::core::option::Option::Some(
                        __cfgy_path
                            .with_key(
                                "description",
                                |__cfgy_path| <String as ::cfgy::FromValue>::from_value(
                                    __cfgy_field,
                                    __cfgy_path,
                                ),
                            )?,
                    )
                }
                ::core::option::Option::None => ::core::option::Option::None,
            },
            timeout_secs: match __cfgy_map.get("timeout_secs") {
                ::core::option::Option::Some(__cfgy_field) => {
                    __cfgy_path
                        .with_key(
                            "timeout_secs",
                            |__cfgy_path| <u64 as ::cfgy::FromValue>::from_value(
                                __cfgy_field,
                                __cfgy_path,
                            ),
                        )?
                }
                ::core::option::Option::None => 30,
            },
        })
    }
}
impl Settings {
    /// Loads `Settings` from `config/settings.yaml`, relative to the current working directory.
    ///
    /// # Errors
    ///
    /// See [`Self::load_from`].
    pub fn load() -> ::core::result::Result<Self, ::cfgy::ConfigError> {
        Self::load_from("config/settings.yaml")
    }
    /// Loads the config file at `path`, choosing its format from the extension unless one was
    /// given in `#[config(format = "...")]`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`](::cfgy::ConfigError::Io) if the file cannot be read,
    /// [`ConfigError::UnknownFormat`](::cfgy::ConfigError::UnknownFormat) if no format can be chosen,
    /// and otherwise any error from [`Self::load_str`].
    pub fn load_from(
        path: impl ::core::convert::AsRef<::std::path::Path>,
    ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
        let __cfgy_path = path.as_ref();
        let __cfgy_source = ::std::fs::read_to_string(__cfgy_path)
            .map_err(|__cfgy_error| {
                ::cfgy::ConfigError::Io {
                    path: __cfgy_path.to_path_buf(),
                    source: __cfgy_error,
                }
            })?;
        let __cfgy_format = ::cfgy::__private::select(
                ::core::option::Option::Some("yaml"),
                __cfgy_path,
                &__cfgy_source,
            )
            .map_err(|__cfgy_error| ::cfgy::ConfigError::UnknownFormat {
                path: __cfgy_path.to_path_buf(),
                message: __cfgy_error.message,
            })?;
        Self::load_str(&__cfgy_source, __cfgy_format)
    }
    /// Parses `source` as `format` and converts it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`](::cfgy::ConfigError::Parse) if `source` is malformed, and a
    /// conversion error if it does not match the struct.
    pub fn load_str(
        source: &str,
        format: &dyn ::cfgy::Format,
    ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
        let __cfgy_value = format
            .parse(source)
            .map_err(|__cfgy_error| ::cfgy::ConfigError::Parse {
                format: format.name(),
                line_col: __cfgy_error.line_col(source),
                message: __cfgy_error.message,
            })?;
        <Self as ::cfgy::FromValue>::from_value(
            &__cfgy_value,
            &mut ::cfgy::PathStack::new(),
        )
    }
}
