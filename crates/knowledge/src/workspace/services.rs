use super::*;
use crate::models::{ModelProfile, ResolvedModel, Service, Transport};

impl Workspace {
    pub fn resolve_model(&self, id: &str) -> Result<ResolvedModel> {
        let registry = self.registry()?;
        let profile = registry
            .models
            .into_iter()
            .find(|model| model.id == id)
            .ok_or("模型尚未绑定，请在库设置中选择已验证的本机模型")?;
        let service = registry
            .services
            .into_iter()
            .find(|service| service.id == profile.service_id)
            .ok_or("模型服务已移除，请重新绑定")?;
        profile.validate(true)?;
        service.validate()?;
        Ok(ResolvedModel { profile, service })
    }
    pub fn service(&self, id: &str) -> Result<Service> {
        self.registry()?
            .services
            .into_iter()
            .find(|service| service.id == id)
            .ok_or("模型服务不存在".into())
    }
    pub fn save_service(&self, mut service: Service) -> Result<Service> {
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        if self.job().state == "running" {
            return Err("请等待当前任务完成后修改模型服务".into());
        }
        if service.id.is_empty() {
            service.id = unique();
        }
        service.validate()?;
        service.revision = unique();
        let mut registry = self.registry()?;
        if let Some(current) = registry
            .services
            .iter_mut()
            .find(|current| current.id == service.id)
        {
            *current = service.clone();
        } else {
            registry.services.push(service.clone());
        }
        let affected = registry
            .models
            .iter()
            .filter(|model| model.service_id == service.id)
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        invalidate(&mut registry, &affected, false);
        self.save(&registry)?;
        self.pool.invalidate();
        Ok(service)
    }
    pub fn validate_model(
        &self,
        mut profile: ModelProfile,
        texts: &[String],
        transport: &dyn Transport,
    ) -> Result<serde_json::Value> {
        profile.validate(false)?;
        if texts.is_empty()
            || texts.len() > 8
            || texts
                .iter()
                .any(|text| text.trim().is_empty() || text.len() > 8192)
        {
            return Err("请输入 1–8 条测试文本，每条不超过 8192 字节".into());
        }
        let service = self.service(&profile.service_id)?;
        let resolved = ResolvedModel {
            profile: profile.clone(),
            service,
        };
        let vectors =
            self.pool
                .embed(&resolved, texts, false, transport, &AtomicBool::new(false))?;
        profile.dimensions = vectors[0].len();
        if vectors
            .iter()
            .any(|vector| vector.len() != profile.dimensions)
        {
            return Err("模型返回了不一致的向量维度".into());
        }
        let query = self
            .pool
            .embed(
                &ResolvedModel {
                    profile: profile.clone(),
                    service: resolved.service,
                },
                &[texts[0].clone()],
                true,
                transport,
                &AtomicBool::new(false),
            )?
            .remove(0);
        let similarities = vectors
            .iter()
            .map(|vector| query.iter().zip(vector).map(|(a, b)| a * b).sum::<f32>())
            .collect::<Vec<_>>();
        Ok(
            serde_json::json!({"profile":profile,"dimensions":query.len(),"similarities":similarities,"tested":true}),
        )
    }
    pub fn save_model(&self, mut profile: ModelProfile) -> Result<ModelProfile> {
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        if self.job().state == "running" {
            return Err("请等待当前任务完成后修改模型".into());
        }
        profile.validate(true)?;
        self.service(&profile.service_id)?;
        if profile.id.is_empty() {
            profile.id = unique();
        }
        if !valid_id(&profile.id) {
            return Err("模型身份无效".into());
        }
        profile.revision = unique();
        let mut registry = self.registry()?;
        if let Some(current) = registry
            .models
            .iter_mut()
            .find(|current| current.id == profile.id)
        {
            *current = profile.clone();
        } else {
            registry.models.push(profile.clone());
        }
        invalidate(&mut registry, std::slice::from_ref(&profile.id), false);
        for library in &mut registry.libraries {
            if library.model.as_deref() == Some(&profile.id) {
                library.model_requirement = Some((&profile).into());
            }
        }
        self.save(&registry)?;
        self.pool.invalidate();
        Ok(profile)
    }
    pub fn remove_service(&self, id: &str, confirmed: bool) -> Result<()> {
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        if !confirmed {
            return Err("请确认移除服务及其模型绑定".into());
        }
        if self.job().state == "running" {
            return Err("请等待当前任务完成".into());
        }
        let mut registry = self.registry()?;
        if !registry.services.iter().any(|service| service.id == id) {
            return Err("模型服务不存在".into());
        }
        let affected = registry
            .models
            .iter()
            .filter(|profile| profile.service_id == id)
            .map(|profile| profile.id.clone())
            .collect::<Vec<_>>();
        invalidate(&mut registry, &affected, true);
        registry.models.retain(|profile| profile.service_id != id);
        registry.services.retain(|service| service.id != id);
        self.save(&registry)?;
        self.pool.invalidate();
        Ok(())
    }
    pub fn remove_model(&self, id: &str, confirmed: bool) -> Result<()> {
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        if !confirmed {
            return Err("请确认移除模型绑定".into());
        }
        if self.job().state == "running" {
            return Err("请等待当前任务完成".into());
        }
        let mut registry = self.registry()?;
        if !registry.models.iter().any(|profile| profile.id == id) {
            return Err("模型不存在".into());
        }
        invalidate(&mut registry, &[id.into()], true);
        registry.models.retain(|profile| profile.id != id);
        self.save(&registry)?;
        self.pool.invalidate();
        Ok(())
    }
}
fn invalidate(registry: &mut Registry, ids: &[String], unbind: bool) {
    for library in &mut registry.libraries {
        if library.model.as_ref().is_some_and(|id| ids.contains(id)) {
            if unbind {
                library.model = None;
            }
            library.vector_revision = None;
            library.revision = unique();
            library.updated_at = now();
        }
    }
}
