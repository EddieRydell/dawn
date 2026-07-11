use super::*;

pub(super) struct DomainResolver<'a> {
    pub(super) loader: &'a mut Loader,
    pub(super) project: &'a mut DawnProject,
}

impl DomainResolver<'_> {
    pub(super) fn resolve_setup(
        &mut self,
        path: &Utf8Path,
        id: &SetupId,
    ) -> Result<(), LoadProjectError> {
        if self.project.setups.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Setup(id.clone()))?;
        let layout_ref = string_field(&document_path, &value, "layout")?;
        let patch_ref = string_field(&document_path, &value, "patch")?;
        let layout = match self.loader.resolve_reference(&document_path, layout_ref)? {
            ResolvedObject::Layout(layout) => layout,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: document_path,
                    range: None,
                    reference: layout_ref.to_string(),
                });
            }
        };
        let patch = match self.loader.resolve_reference(&document_path, patch_ref)? {
            ResolvedObject::Patch(patch) => patch,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: None,
                    reference: patch_ref.to_string(),
                });
            }
        };
        let controllers = sequence_field(&document_path, &value, "controllers")?
            .iter()
            .map(|name| {
                Identifier::new(name.to_string())
                    .map(|name| {
                        ControllerId(SourceIdentity::new(
                            document_path.clone(),
                            name.as_str().to_string(),
                        ))
                    })
                    .map_err(|_| LoadProjectError::InvalidReference {
                        path: document_path.clone(),
                        range: source_range_for_scalar(&document_path, name),
                        reference: name.to_string(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.project.setups.insert(
            id.clone(),
            Setup {
                id: id.clone(),
                layout: layout.clone(),
                patch: patch.clone(),
                controllers: controllers.clone(),
            },
        );
        self.resolve_layout(&document_path, &layout)?;
        self.resolve_patch(&document_path, &patch)?;
        for controller in controllers {
            self.resolve_controller(&document_path, &controller)?;
        }
        Ok(())
    }

    pub(super) fn resolve_controller(
        &mut self,
        path: &Utf8Path,
        id: &ControllerId,
    ) -> Result<(), LoadProjectError> {
        if self.project.controllers.contains_key(id) {
            return Ok(());
        }
        let (_, _, value) = self
            .loader
            .object_value(&ResolvedObject::Controller(id.clone()))?;
        let protocol = match string_field(path, &value, "protocol")? {
            "sacn" => Protocol::E131,
            "artnet" => Protocol::Artnet,
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
                    message: format!("unsupported controller protocol `{other}`"),
                });
            }
        };
        let address = optional_string_field(&value, "destination")
            .map(parse_controller_address)
            .transpose()
            .map_err(|message| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message,
            })?;
        let output = required_mapping(path, &value, "output")?;
        let output_value = Value::Mapping(output);
        let output_type = string_field(path, &output_value, "type")?;
        let channel_order =
            parse_channel_order(string_field(path, &output_value, "channel_order")?).ok_or_else(
                || LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
                    message: "invalid channel order".to_string(),
                },
            )?;
        let outputs = match output_type {
            "linear_rgb" => {
                let output_count = u32_field(path, &output_value, "output_count")?;
                let pixels = usize_field(path, &output_value, "pixels_per_output")?;
                let first_universe = u32_field(path, &output_value, "first_universe")?;
                (0..output_count)
                    .map(|index| ControllerOutput {
                        channel_order: channel_order.clone(),
                        pixels,
                        first_universe: first_universe + index,
                    })
                    .collect()
            }
            "patched_dmx" => sequence_values(path, &output_value, "universes")?
                .iter()
                .map(|universe| {
                    let range = string_field(path, universe, "range")?;
                    let slots = parse_slot_range(range).ok_or_else(|| {
                        LoadProjectError::InvalidDocument {
                            path: path.to_path_buf(),
                            range: None,
                            message: format!("invalid universe range `{range}`"),
                        }
                    })?;
                    Ok(ControllerOutput {
                        channel_order: channel_order.clone(),
                        pixels: slots / 3,
                        first_universe: u32_field(path, universe, "id")?,
                    })
                })
                .collect::<Result<Vec<_>, LoadProjectError>>()?,
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
                    message: format!("unsupported controller output type `{other}`"),
                });
            }
        };
        let definition = ControllerDefinition {
            protocol,
            address,
            outputs,
        };
        self.project
            .definitions
            .controllers
            .insert(ControllerDefinitionId(id.0.clone()), definition.clone());
        self.project.controllers.insert(id.clone(), definition);
        Ok(())
    }

    pub(super) fn resolve_layout(
        &mut self,
        path: &Utf8Path,
        id: &LayoutId,
    ) -> Result<(), LoadProjectError> {
        if self.project.layouts.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Layout(id.clone()))?;
        let target_order = optional_sequence(&value, "target_order")
            .unwrap_or_default()
            .iter()
            .map(|target| parse_layout_target(&document_path, target))
            .collect::<Result<Vec<_>, _>>()?;
        let fixtures = optional_sequence(&value, "fixtures")
            .unwrap_or_default()
            .iter()
            .map(|fixture| self.parse_fixture_inst(&document_path, fixture))
            .collect::<Result<Vec<_>, _>>()?;
        let groups = optional_sequence(&value, "groups")
            .unwrap_or_default()
            .iter()
            .map(|group| parse_fixture_group(&document_path, group))
            .collect::<Result<Vec<_>, _>>()?;
        for fixture in &fixtures {
            self.resolve_fixture_definition(&document_path, &fixture.definition)?;
        }
        self.project.layouts.insert(
            id.clone(),
            Layout {
                id: id.clone(),
                target_order,
                fixtures,
                groups,
            },
        );
        let _ = path;
        Ok(())
    }

    pub(super) fn parse_fixture_inst(
        &self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<FixtureInst, LoadProjectError> {
        let id = FixtureInstanceId(u32_field(path, value, "id")?);
        let name = string_field(path, value, "name")?.to_string();
        let fixture_ref = string_field(path, value, "fixture")?;
        let definition = match self.loader.resolve_reference(path, fixture_ref)? {
            ResolvedObject::FixtureDefinition(definition) => definition,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: None,
                    reference: fixture_ref.to_string(),
                });
            }
        };
        let transform = optional_mapping_ref(value, "transform");
        let position = transform
            .and_then(|mapping| mapping.get(Value::String("position".to_string())))
            .map(parse_point3)
            .transpose()?
            .unwrap_or_default();
        let rotation = transform
            .and_then(|mapping| mapping.get(Value::String("rotation".to_string())))
            .map(parse_rotation3)
            .transpose()?
            .unwrap_or_default();
        let scale = transform
            .and_then(|mapping| mapping.get(Value::String("scale".to_string())))
            .map(parse_scale3)
            .transpose()?
            .unwrap_or_default();
        Ok(FixtureInst {
            id,
            name,
            definition,
            position,
            rotation,
            scale,
        })
    }

    pub(super) fn resolve_fixture_definition(
        &mut self,
        path: &Utf8Path,
        id: &FixtureDefinitionId,
    ) -> Result<(), LoadProjectError> {
        if !self
            .project
            .definitions
            .fixtures
            .definitions
            .contains_key(id)
        {
            return Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: None,
                reference: id.0.object().to_string(),
            });
        }
        Ok(())
    }

    pub(super) fn resolve_patch(
        &mut self,
        _path: &Utf8Path,
        id: &PatchId,
    ) -> Result<(), LoadProjectError> {
        if self.project.patches.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Patch(id.clone()))?;
        let routes = optional_sequence(&value, "routes")
            .unwrap_or_default()
            .iter()
            .map(|route| self.parse_patch_route(&document_path, route))
            .collect::<Result<Vec<_>, _>>()?;
        self.project.patches.insert(
            id.clone(),
            Patch {
                id: id.clone(),
                routes,
            },
        );
        Ok(())
    }

    pub(super) fn parse_patch_route(
        &self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<PatchRoute, LoadProjectError> {
        let controller_ref = string_field(path, value, "controller")?;
        let controller = match self.loader.resolve_reference(path, controller_ref)? {
            ResolvedObject::Controller(controller) => controller,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: None,
                    reference: controller_ref.to_string(),
                });
            }
        };
        let output = optional_field(value, "output")
            .and_then(Value::as_u64)
            .map(|value| value as u32)
            .or_else(|| {
                optional_field(value, "universe")
                    .and_then(Value::as_u64)
                    .map(|value| value.saturating_sub(1) as u32)
            })
            .ok_or_else(|| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "patch route must contain output or universe".to_string(),
            })?;
        let start_channel_offset = optional_field(value, "start_channel_offset")
            .and_then(Value::as_u64)
            .map(|value| value as u32)
            .or_else(|| {
                optional_field(value, "start")
                    .and_then(Value::as_u64)
                    .map(|value| value.saturating_sub(1) as u32)
            })
            .ok_or_else(|| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "patch route must contain start_channel_offset or start".to_string(),
            })?;
        Ok(PatchRoute {
            fixture: FixtureInstanceId(u32_field(path, value, "fixture")?),
            fixture_pixels: PixelRange {
                start: optional_field(value, "fixture_pixel_start")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                count: optional_field(value, "fixture_pixel_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
            },
            controller,
            output: ControllerOutputIndex(output),
            start_channel_offset,
        })
    }

    pub(super) fn resolve_sequence(
        &mut self,
        path: &Utf8Path,
        id: &SequenceId,
    ) -> Result<(), LoadProjectError> {
        if self.project.sequences.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Sequence(id.clone()))?;
        let duration =
            parse_duration(string_field(&document_path, &value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    &document_path,
                    source_range_for_field_value(&document_path, &value, "duration"),
                )
            })?;
        let audio = self.parse_audio(&document_path, &value)?;
        let mark_collections = optional_sequence(&value, "mark_collections")
            .unwrap_or_default()
            .iter()
            .map(|collection| parse_mark_collection(&document_path, collection))
            .collect::<Result<Vec<_>, _>>()?;
        let layers = sequence_values(&document_path, &value, "layers")?
            .iter()
            .map(|layer| parse_sequence_layer(&document_path, layer))
            .collect::<Result<Vec<_>, _>>()?;
        let effects = sequence_values(&document_path, &value, "effects")?
            .iter()
            .map(|effect| self.parse_sequence_effect(&document_path, effect))
            .collect::<Result<Vec<_>, _>>()?;
        let composition_graph = self.parse_composition_graph(
            &document_path,
            required_field(&document_path, &value, "composition_graph")?,
        )?;
        let automation_clips = optional_sequence(&value, "automation_clips")
            .unwrap_or_default()
            .iter()
            .map(|clip| self.parse_automation_clip(&document_path, clip))
            .collect::<Result<Vec<_>, _>>()?;
        self.project.sequences.insert(
            id.clone(),
            Sequence {
                id: id.clone(),
                duration,
                frame_rate: u32_field(&document_path, &value, "frame_rate")?,
                audio,
                mark_collections,
                layers,
                effects,
                composition_graph,
                automation_clips,
            },
        );
        let _ = path;
        Ok(())
    }

    pub(super) fn parse_audio(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<SequenceAudio, LoadProjectError> {
        let Some(audio) = optional_field(value, "audio") else {
            return Ok(SequenceAudio::None);
        };
        if matches!(audio, Value::Null) {
            return Ok(SequenceAudio::None);
        }
        let Some(audio_path) = audio.as_str() else {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "audio must be null or a path string".to_string(),
            });
        };
        let document_absolute = self.loader.source_root.join(path);
        let document_dir = document_absolute
            .parent()
            .unwrap_or(&self.loader.source_root);
        let absolute = document_dir
            .join(audio_path)
            .canonicalize_utf8()
            .map_err(|source| LoadProjectError::Io {
                path: document_dir.join(audio_path),
                source,
            })?;
        if !absolute.is_file() {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: format!("audio asset does not exist: {audio_path}"),
            });
        }
        let relative = match relative_path(&self.loader.source_root, &absolute) {
            Ok(relative) => relative,
            Err(_) if !Utf8Path::new(audio_path).is_absolute() => Utf8PathBuf::from(audio_path),
            Err(error) => return Err(error),
        };
        if let Some(existing) = self
            .loader
            .referenced_assets
            .iter()
            .find(|asset| asset.relative_path == relative)
        {
            return Ok(SequenceAudio::Asset(existing.id.clone()));
        }
        let id = AssetId(self.loader.next_asset_id);
        self.loader.next_asset_id += 1;
        self.loader.referenced_assets.push(ReferencedAsset {
            id: id.clone(),
            relative_path: relative,
            absolute_path: absolute,
        });
        Ok(SequenceAudio::Asset(id))
    }

    pub(super) fn parse_sequence_effect(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectInst, LoadProjectError> {
        let effect = self.parse_effect_clip(path, value)?;
        Ok(EffectInst {
            id: EffectInstId(u32_field(path, value, "id")?),
            layer_id: SequenceLayerId(u32_field(path, value, "layer_id")?),
            start: parse_duration_as_time(string_field(path, value, "start")?).map_err(
                |error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "start"),
                    )
                },
            )?,
            duration: parse_duration(string_field(path, value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, value, "duration"),
                )
            })?,
            target: parse_effect_target(path, required_field(path, value, "target")?)?,
            scope: parse_effect_scope(path, value)?,
            definition: effect.definition,
            param_overrides: effect.param_overrides,
        })
    }

    pub(super) fn parse_composition_graph(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<SequenceCompositionGraph, LoadProjectError> {
        let graph = SequenceCompositionGraph {
            nodes: sequence_values(path, value, "nodes")?
                .iter()
                .map(|node| self.parse_composition_graph_node(path, node))
                .collect::<Result<Vec<_>, _>>()?,
            edges: sequence_values(path, value, "edges")?
                .iter()
                .map(|edge| parse_graph_edge(path, edge))
                .collect::<Result<Vec<_>, _>>()?,
        };
        validate_composition_graph(&graph, &self.project.definitions.operators).map_err(
            |error| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: error.message,
            },
        )?;
        Ok(graph)
    }

    pub(super) fn parse_composition_graph_node(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<CompositionGraphNode, LoadProjectError> {
        let kind = match string_field(path, value, "type")? {
            "layer" => CompositionGraphNodeKind::Layer {
                layer_id: SequenceLayerId(u32_field(path, value, "layer_id")?),
            },
            "operator" => CompositionGraphNodeKind::Operator(GraphOperatorNode {
                operator: self.parse_graph_operator_ref(path, value)?,
                params: self.parse_graph_operator_params(path, value)?,
            }),
            "output" => CompositionGraphNodeKind::Output,
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "type"),
                    message: format!("unsupported composition graph node type `{other}`"),
                });
            }
        };
        Ok(CompositionGraphNode {
            id: CompositionGraphNodeId(u32_field(path, value, "id")?),
            position: parse_graph_position(path, required_field(path, value, "position")?)?,
            kind,
        })
    }

    pub(super) fn parse_effect_clip(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectClip, LoadProjectError> {
        let script_ref = string_field(path, value, "script")?;
        let definition = match self.loader.resolve_reference(path, script_ref)? {
            ResolvedObject::EffectDefinition(definition) => definition,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: None,
                    reference: script_ref.to_string(),
                });
            }
        };
        self.resolve_effect_definition(&definition)?;
        let params = optional_mapping(value, "params")
            .map(|mapping| {
                mapping
                    .iter()
                    .map(|(key, value)| {
                        let key =
                            key.as_str()
                                .ok_or_else(|| LoadProjectError::InvalidDocument {
                                    path: path.to_path_buf(),
                                    range: None,
                                    message: "effect param keys must be strings".to_string(),
                                })?;
                        let identifier = Identifier::new(key.to_string()).map_err(|_| {
                            LoadProjectError::InvalidDocument {
                                path: path.to_path_buf(),
                                range: None,
                                message: format!("invalid effect param name `{key}`"),
                            }
                        })?;
                        Ok((identifier, self.parse_effect_param(path, value)?))
                    })
                    .collect::<Result<IndexMap<_, _>, LoadProjectError>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(EffectClip {
            definition,
            param_overrides: params,
        })
    }

    pub(super) fn parse_graph_operator_params(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<IndexMap<Identifier, EffectParamValue>, LoadProjectError> {
        optional_mapping(value, "params")
            .map(|mapping| {
                mapping
                    .iter()
                    .map(|(key, value)| {
                        let key =
                            key.as_str()
                                .ok_or_else(|| LoadProjectError::InvalidDocument {
                                    path: path.to_path_buf(),
                                    range: None,
                                    message: "operator param keys must be strings".to_string(),
                                })?;
                        let identifier = Identifier::new(key.to_string()).map_err(|_| {
                            LoadProjectError::InvalidDocument {
                                path: path.to_path_buf(),
                                range: None,
                                message: format!("invalid operator param name `{key}`"),
                            }
                        })?;
                        Ok((identifier, self.parse_effect_param(path, value)?))
                    })
                    .collect::<Result<IndexMap<_, _>, LoadProjectError>>()
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    pub(super) fn parse_graph_operator_ref(
        &self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<OperatorRef, LoadProjectError> {
        let name = string_field(path, value, "operator")?;
        if let Some(builtin) = BuiltinOperator::from_source_name(name) {
            return Ok(OperatorRef::Builtin(builtin));
        }
        match self.loader.resolve_reference(path, name)? {
            ResolvedObject::OperatorDefinition(id) => Ok(OperatorRef::Custom(id)),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "operator"),
                reference: name.to_string(),
            }),
        }
    }

    pub(super) fn resolve_effect_definition(
        &mut self,
        id: &EffectDefinitionId,
    ) -> Result<(), LoadProjectError> {
        if !self
            .project
            .definitions
            .effects
            .definitions
            .contains_key(id)
        {
            return Err(LoadProjectError::InvalidReference {
                path: self.loader.entrypoint.clone(),
                range: None,
                reference: id.0.object().to_string(),
            });
        }
        Ok(())
    }

    pub(super) fn parse_effect_param(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectParamValue, LoadProjectError> {
        match string_field(path, value, "type")? {
            "integer" => Ok(EffectParamValue::Int(i64_field(path, value, "value")?)),
            "float" => Ok(EffectParamValue::Float(f64_field(path, value, "value")?)),
            "bool" => Ok(EffectParamValue::Bool(bool_field(path, value, "value")?)),
            "color" => Ok(EffectParamValue::Color(
                parse_color(string_field(path, value, "value")?).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "value"),
                    )
                })?,
            )),
            "enum" => Ok(EffectParamValue::Enum(
                Identifier::new(string_field(path, value, "value")?.to_string()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "invalid enum value".to_string(),
                    }
                })?,
            )),
            "marks" => Ok(EffectParamValue::Marks(MarkCollectionKey {
                name: string_field(path, value, "key")?.to_string(),
            })),
            "curve" => Ok(EffectParamValue::Curve(
                self.parse_curve_source(path, required_field(path, value, "curve")?)?,
            )),
            "array" => {
                let values = sequence_values(path, value, "values")?
                    .iter()
                    .map(|item| self.parse_array_item(path, item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(EffectParamValue::Array(values))
            }
            other => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: format!("unsupported effect param type `{other}`"),
            }),
        }
    }

    pub(super) fn parse_array_item(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectParamValue, LoadProjectError> {
        if optional_field(value, "type").is_some() {
            return self.parse_effect_param(path, value);
        }
        let curve = required_field(path, value, "curve")?;
        Ok(EffectParamValue::Curve(
            self.parse_curve_source(path, curve)?,
        ))
    }

    pub(super) fn parse_curve_source(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<CurveSource, LoadProjectError> {
        if let Some(reference) = value.as_str() {
            let id = match self.loader.resolve_reference(path, reference)? {
                ResolvedObject::Curve(curve) => curve,
                _ => {
                    return Err(LoadProjectError::InvalidReference {
                        path: path.to_path_buf(),
                        range: None,
                        reference: reference.to_string(),
                    });
                }
            };
            self.resolve_curve(path, &id)?;
            return Ok(CurveSource::Reference(id));
        }
        if let Some(curve_value) = optional_field(value, "curve") {
            return self.parse_curve_source(path, curve_value);
        }
        Ok(CurveSource::Inline(parse_curve(path, value)?))
    }

    pub(super) fn resolve_curve(
        &mut self,
        path: &Utf8Path,
        id: &CurveId,
    ) -> Result<(), LoadProjectError> {
        if !self.project.definitions.curves.definitions.contains_key(id) {
            return Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: None,
                reference: id.0.object().to_string(),
            });
        }
        Ok(())
    }

    pub(super) fn parse_automation_clip(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<AutomationClip, LoadProjectError> {
        let bindings = sequence_values(path, value, "bindings")?
            .iter()
            .map(|binding| parse_automation_binding(path, binding))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = IndexSet::new();
        for binding in &bindings {
            if !seen.insert(binding.target.clone()) {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "bindings"),
                    message: "automation clip has duplicate bindings for a parameter".to_string(),
                });
            }
        }
        Ok(AutomationClip {
            id: AutomationClipId(u32_field(path, value, "id")?),
            start: parse_duration_as_time(string_field(path, value, "start")?).map_err(
                |error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "start"),
                    )
                },
            )?,
            duration: parse_duration(string_field(path, value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, value, "duration"),
                )
            })?,
            anchor_lane_index: u32_field(path, value, "anchor_lane_index")?,
            lane_index: u32_field(path, value, "lane_index")?,
            curve: parse_automation_curve(path, required_field(path, value, "curve")?)?,
            bindings,
        })
    }
}
